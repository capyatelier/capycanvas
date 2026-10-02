use crate::localization::{Localizer, UiLanguage, MessageId};
use crate::shortcuts::{ShortcutSection, ShortcutLabel, shortcut_brush_detail, shortcut_cannot_hold, shortcut_context_choice, shortcut_context_empty, shortcut_context_summary, shortcut_hold_help, shortcut_key_context, shortcut_key_unassigned, shortcut_list_and};
use crate::shortcuts::{BindingScope, KeyChord, SHORTCUT_SECTIONS, ShortcutDefinition, ShortcutRow, definitions, hold_id};
use crate::shortcuts::HoldKey;
use crate::{CommandId, GESTURE_TRIGGERS, GestureTrigger, ModifierKeyAction, ModifierKeyEditor, Platform, Settings, ToolCategory, UiAction};

use serde::{Deserialize, Serialize};

pub(crate) const CONTEXTS: [ToolCategory; 11] = [
    ToolCategory::Drawing,
    ToolCategory::Erasing,
    ToolCategory::Blending,
    ToolCategory::Warping,
    ToolCategory::Retouching,
    ToolCategory::Selection,
    ToolCategory::FillGradient,
    ToolCategory::ShapesRulers,
    ToolCategory::MoveTransform,
    ToolCategory::ColorSampling,
    ToolCategory::Navigation,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutShow {
    #[default]
    All,
    Assigned,
    Customized,
}
impl ShortcutShow {
    pub const ALL: [Self; 3] = [Self::All, Self::Assigned, Self::Customized];
    pub fn label(self) -> String { self.localized_label(&Localizer::shared(UiLanguage::English)) }
    pub fn localized_label(self, l: &Localizer) -> String {
        match self {
            Self::All => l.text(MessageId::SHORTCUT_ALL_ACTIONS).to_string(),
            Self::Assigned => l.text(MessageId::SHORTCUT_WITH_SHORTCUTS).to_string(),
            Self::Customized => l.text(MessageId::SHORTCUT_CUSTOMIZED).to_string(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ShortcutPageState {
    pub category: Option<String>,
    pub context: Option<ToolCategory>,
    pub show: ShortcutShow,
    pub key: Option<KeyChord>,
    pub picker: Option<(String, String)>,
    pub modifier_picker: Option<(KeyChord, Option<ToolCategory>)>,
    pub pen_picker: Option<(String, Option<ToolCategory>)>,
}

pub const MODIFIER_SECTION: &str = "Modifier keys";

#[derive(Clone, Debug, Serialize)]
pub struct ModifierKeyRow {
    pub key: KeyChord,
    pub label: String,
    pub action: String,
    pub detail: String,
    pub modified: bool,
    pub visible: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutContextChoice {
    pub category: Option<ToolCategory>,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutShowChoice {
    pub show: ShortcutShow,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutCategoryView {
    pub id: String,
    pub label: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShortcutEmpty {
    pub title: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TriggerRow {
    pub id: String,
    pub section: String,
    pub label: String,
    pub action: String,
    pub detail: String,
    pub modified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PenButtonEditor {
    pub trigger: String,
    pub label: String,
    pub per_tool: bool,
    pub actions: Vec<ModifierKeyAction>,
    pub modified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PickerAction {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub selected: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PickerSection {
    pub title: String,
    pub actions: Vec<PickerAction>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActionPickerView {
    pub trigger: String,
    pub title: String,
    pub description: String,
    pub query: String,
    pub modified: bool,
    pub nothing: bool,
    pub sections: Vec<PickerSection>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutPageView {
    pub contexts: Vec<ShortcutContextChoice>,
    pub context: Option<ToolCategory>,
    pub shows: Vec<ShortcutShowChoice>,
    pub show: ShortcutShow,
    pub key: Option<String>,
    pub filtering: bool,
    pub category: Option<String>,
    pub categories: Vec<ShortcutCategoryView>,
    pub empty: Option<ShortcutEmpty>,
    pub triggers: Vec<TriggerRow>,
    pub picker: Option<ActionPickerView>,
    pub modifiers: Vec<ModifierKeyRow>,
}

pub(crate) fn scope_label_localized(scope: &BindingScope, l: &Localizer) -> String {
    match scope {
        BindingScope::Application => String::new(),
        BindingScope::Canvas => l.text(MessageId::SHORTCUT_CANVAS).to_string(),
        BindingScope::Tools { categories } => categories.iter().map(|c| c.localized_label(l).to_string()).collect::<Vec<_>>().join(", "),
    }
}

pub(crate) fn triggers(platform: Platform) -> impl Iterator<Item = &'static GestureTrigger> {
    GESTURE_TRIGGERS
        .iter()
        .filter(move |t| if t.held { platform.pen_buttons() } else { platform.touch_gestures() })
        .filter(move |t| t.id != "pen.button.tertiary" || platform == Platform::Gtk)
}

fn action_label(all: &[(ShortcutDefinition, ShortcutSection)], id: &str, l: &Localizer) -> String {
    if id.is_empty() {
        return l.text(MessageId::SHORTCUT_NOTHING).to_string();
    }
    all.iter().find(|(d, _)| d.id == id).map_or_else(|| id.to_string(), |(d, _)| d.label.resolve(l))
}

fn matches(query: &str, text: &str) -> bool {
    query.is_empty() || crate::search::normalize(text).contains(query)
}

fn brush_tool(definition: &ShortcutDefinition, l: &Localizer) -> Option<String> {
    let crate::shortcuts::ShortcutAction::Action { action } = &definition.action else {
        return None;
    };
    let UiAction::SelectBrush { id } = **action else {
        return None;
    };
    let tool = crate::tools::group(id).tool();
    CommandId::TOOLS.into_iter().find(|c| c.paint_tool() == Some(tool)).map(|c| c.localized_label(l).to_string())
}

fn detail(definition: &ShortcutDefinition, grouped: bool, shared_label: bool, l: &Localizer) -> String {
    if definition.id.starts_with("tools.") {
        return l.text(MessageId::SHORTCUT_CYCLE_HELP).to_string();
    }
    match brush_tool(definition, l) {
        Some(tool) if !grouped => shortcut_brush_detail(l, tool),
        Some(_) => String::new(),
        None if shared_label && definition.id.starts_with("command.") => l.text(MessageId::SHORTCUT_CURRENT_BRUSH_HELP).to_string(),
        None => String::new(),
    }
}

fn filtering(state: &ShortcutPageState, query: &str) -> bool {
    !query.trim().is_empty() || state.key.is_some() || state.show != ShortcutShow::All || state.context.is_some()
}

fn key_matches(chord: &KeyChord, query: &str, platform: Platform) -> bool {
    chord.key.eq_ignore_ascii_case(query)
        || chord.label_parts(platform).last().is_some_and(|key| key.eq_ignore_ascii_case(query))
}

fn tool_phrase(category: ToolCategory, l: &Localizer) -> std::sync::Arc<str> {
    l.text(match category {
        ToolCategory::Drawing => MessageId::SHORTCUT_CONTEXT_DRAWING,
        ToolCategory::Erasing => MessageId::SHORTCUT_CONTEXT_ERASING,
        ToolCategory::Blending => MessageId::SHORTCUT_CONTEXT_BLENDING,
        ToolCategory::Warping => MessageId::SHORTCUT_CONTEXT_WARPING,
        ToolCategory::Retouching => MessageId::SHORTCUT_CONTEXT_RETOUCHING,
        ToolCategory::Selection => MessageId::SHORTCUT_CONTEXT_SELECTION,
        ToolCategory::FillGradient => MessageId::SHORTCUT_CONTEXT_FILL_GRADIENT,
        ToolCategory::ShapesRulers => MessageId::SHORTCUT_CONTEXT_SHAPES_RULERS,
        ToolCategory::MoveTransform => MessageId::SHORTCUT_CONTEXT_MOVE_TRANSFORM,
        ToolCategory::ColorSampling => MessageId::SHORTCUT_CONTEXT_COLOR_SAMPLING,
        ToolCategory::Navigation => MessageId::SHORTCUT_CONTEXT_NAVIGATION,
    })
}

fn tools_phrase(categories: &[ToolCategory], l: &Localizer) -> String {
    let names: Vec<_> = categories.iter().map(|c| tool_phrase(*c, l).to_string()).collect();
    match names.as_slice() {
        [one] => one.clone(),
        [rest @ .., last] => shortcut_list_and(l, rest.join(", "), last.clone()),
        [] => String::new(),
    }
}

fn held_label(all: &[(ShortcutDefinition, ShortcutSection)], target: &str, l: &Localizer) -> String {
    if target.is_empty() {
        return l.text(MessageId::SHORTCUT_NOTHING).to_string();
    }
    hold_id(target)
        .and_then(|id| all.iter().find(|(d, _)| d.id == id))
        .map_or_else(|| target.to_string(), |(d, _)| match &d.label {
            ShortcutLabel::Held(base) => base.resolve(l),
            _ => d.label.resolve(l),
        })
}

/// Held actions read as what they do ("Sample color"); others keep their name.
fn target_label(all: &[(ShortcutDefinition, ShortcutSection)], target: &str, l: &Localizer) -> String {
    match hold_id(target) {
        Some(_) => held_label(all, target, l),
        None if target.is_empty() => l.text(MessageId::SHORTCUT_NOTHING).to_string(),
        None => all.iter().find(|(d, _)| d.id == target).map_or_else(|| target.to_string(), |(d, _)| d.label.resolve(l)),
    }
}

fn actions_summary(all: &[(ShortcutDefinition, ShortcutSection)], actions: &std::collections::BTreeMap<ToolCategory, String>, l: &Localizer) -> (String, String) {
    let mut targets: Vec<&String> = actions.values().collect();
    targets.sort();
    targets.dedup();
    match targets.as_slice() {
        [] => (l.text(MessageId::SHORTCUT_NOTHING).to_string(), String::new()),
        [one] if actions.len() == CONTEXTS.len() => (target_label(all, one, l), String::new()),
        [one] => {
            let tools: Vec<_> = actions.keys().copied().collect();
            (target_label(all, one, l), shortcut_context_summary(l, tools_phrase(&tools, l)))
        }
        many => (
            l.text(MessageId::SHORTCUT_CONTEXT_DEPENDENT).to_string(),
            many.iter().map(|t| target_label(all, t, l)).collect::<Vec<_>>().join(" · "),
        ),
    }
}

fn per_tool_rows(
    all: &[(ShortcutDefinition, ShortcutSection)],
    actions: &std::collections::BTreeMap<ToolCategory, String>,
    per_tool: bool, l: &Localizer) -> (bool, Vec<ModifierKeyAction>) {
    let uniform = CONTEXTS.iter().all(|c| actions.get(c) == actions.get(&CONTEXTS[0]));
    let per_tool = per_tool || !uniform;
    let rows = if per_tool {
        CONTEXTS
            .into_iter()
            .map(|category| ModifierKeyAction {
                category: Some(category),
                label: shortcut_context_choice(l, category.localized_label(l).to_string()),
                action: target_label(all, actions.get(&category).map_or("", String::as_str), l),
            })
            .collect()
    } else {
        vec![ModifierKeyAction {
            category: None,
            label: l.text(MessageId::SHORTCUT_ACTION).to_string(),
            action: target_label(all, actions.get(&CONTEXTS[0]).map_or("", String::as_str), l),
        }]
    };
    (per_tool, rows)
}

fn apply_action(actions: &mut std::collections::BTreeMap<ToolCategory, String>, category: Option<ToolCategory>, action: &str) {
    for category in category.map_or_else(|| CONTEXTS.to_vec(), |c| vec![c]) {
        if action.is_empty() {
            actions.remove(&category);
        } else {
            actions.insert(category, action.to_string());
        }
    }
}

fn most_common(actions: &std::collections::BTreeMap<ToolCategory, String>) -> String {
    let mut counts = std::collections::BTreeMap::<&String, usize>::new();
    for target in actions.values() {
        *counts.entry(target).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(t, _)| t.clone()).unwrap_or_default()
}

pub(crate) fn pen_editor_localized(settings: &Settings, platform: Platform, trigger: &str, per_tool: bool, l: &Localizer) -> Option<PenButtonEditor> {
    let definition = GESTURE_TRIGGERS.iter().find(|t| t.id == trigger && t.held)?;
    let all = definitions(platform);
    let (per_tool, actions) = per_tool_rows(&all, &settings.pen_actions(trigger), per_tool, l);
    Some(PenButtonEditor {
        trigger: trigger.into(),
        label: definition.localized_label(l),
        per_tool,
        actions,
        modified: settings.pen_buttons.contains_key(trigger) || settings.gestures.contains_key(trigger),
    })
}

pub(crate) fn set_pen_button_localized(settings: &mut Settings, platform: Platform, trigger: &str, category: Option<ToolCategory>, action: &str, l: &Localizer) -> Result<(), String> {
    if !GESTURE_TRIGGERS.iter().any(|t| t.id == trigger && t.held) {
        return Err(l.text(MessageId::SHORTCUT_UNKNOWN_PEN_BUTTON).to_string());
    }
    if !action.is_empty() && !definitions(platform).iter().any(|(d, _)| d.id == action && d.target.is_none()) {
        return Err(l.text(MessageId::SHORTCUT_UNKNOWN_ACTION).to_string());
    }
    let mut actions = settings.pen_actions(trigger);
    apply_action(&mut actions, category, action);
    settings.gestures.remove(trigger);
    let default = {
        let mut defaults = settings.clone();
        defaults.pen_buttons.remove(trigger);
        defaults.pen_actions(trigger)
    };
    if actions == default {
        settings.pen_buttons.remove(trigger);
    } else {
        settings.pen_buttons.insert(trigger.into(), actions);
    }
    Ok(())
}

pub(crate) fn unify_pen_button_localized(settings: &mut Settings, platform: Platform, trigger: &str, l: &Localizer) -> Result<(), String> {
    let common = most_common(&settings.pen_actions(trigger));
    set_pen_button_localized(settings, platform, trigger, None, &common, l)
}

pub(crate) fn modifier_rows_localized(settings: &Settings, platform: Platform, state: &ShortcutPageState, query: &str, l: &Localizer) -> Vec<ModifierKeyRow> {
    let all = definitions(platform);
    let defaults = settings.default_hold_keys(platform);
    let filtering = filtering(state, query);
    let single = crate::search::ascii_key_query(query.trim());
    let query = crate::search::normalize(query.trim());
    settings
        .hold_keys(platform)
        .into_iter()
        .map(|hold| {
            let summary = |localizer: &Localizer| match state.context.and_then(|category| hold.actions.get(&category)) {
                Some(target) => (target_label(&all, target, localizer), String::new()),
                None => actions_summary(&all, &hold.actions, localizer),
            };
            let (action, detail) = summary(l);
            let label = hold.key.localized_label(platform, l);
            let modified = !defaults.contains(&hold);
            let bound = match state.context {
                Some(category) => hold.actions.contains_key(&category),
                None => !hold.actions.is_empty(),
            };
            let matched = match &state.key {
                Some(key) => *key == hold.key,
                None if single => false,
                None => {
                    let english = Localizer::shared(UiLanguage::English);
                    let (canonical_action, canonical_detail) = summary(&english);
                    matches(&query, &format!("{} {label} {action} {detail} {} {} {canonical_action} {canonical_detail}",
                        l.text(MessageId::SHORTCUT_MODIFIER_KEYS), english.text(MessageId::SHORTCUT_MODIFIER_KEYS), hold.key.localized_label(platform, &english)))
                },
            };
            let visible = if filtering {
                matched
                    && match state.show {
                        ShortcutShow::All => true,
                        ShortcutShow::Assigned => bound,
                        ShortcutShow::Customized => modified,
                    }
                    && (state.context.is_none() || bound)
            } else {
                state.category.as_deref() == Some(MODIFIER_SECTION)
            };
            ModifierKeyRow { key: hold.key, label, action, detail, modified, visible }
        })
        .collect()
}

pub(crate) fn modifier_editor_localized(settings: &Settings, platform: Platform, key: &KeyChord, per_tool: bool, l: &Localizer) -> ModifierKeyEditor {
    let all = definitions(platform);
    let hold = settings.hold_keys(platform).into_iter().find(|h| h.key == *key).unwrap_or(HoldKey { key: key.clone(), actions: Default::default() });
    let (per_tool, actions) = per_tool_rows(&all, &hold.actions, per_tool, l);
    ModifierKeyEditor {
        label: key.localized_label(platform, l),
        modified: settings.default_hold_keys(platform).iter().any(|d| d.key == hold.key && *d != hold),
        key: key.clone(),
        per_tool,
        actions,
    }
}

pub(crate) fn store_modifiers(settings: &mut Settings, platform: Platform, table: Vec<HoldKey>) {
    settings.hold_keys = (table != settings.default_hold_keys(platform)).then_some(table);
}

pub(crate) fn set_modifier_localized(settings: &mut Settings, platform: Platform, key: &KeyChord, category: Option<ToolCategory>, action: &str, l: &Localizer) -> Result<(), String> {
    if !action.is_empty() && !hold_id(action).is_some_and(|id| definitions(platform).iter().any(|(d, _)| d.id == id)) {
        return Err(l.text(MessageId::SHORTCUT_HELD_ACTION_INVALID).to_string());
    }
    let mut table = settings.hold_keys(platform);
    let hold = table.iter_mut().find(|h| h.key == *key).ok_or_else(|| l.text(MessageId::SHORTCUT_UNKNOWN_MODIFIER_KEY).to_string())?;
    apply_action(&mut hold.actions, category, action);
    store_modifiers(settings, platform, table);
    Ok(())
}

pub(crate) fn unify_modifier_localized(settings: &mut Settings, platform: Platform, key: &KeyChord, l: &Localizer) -> Result<(), String> {
    let hold = settings.hold_keys(platform).into_iter().find(|h| h.key == *key).ok_or_else(|| l.text(MessageId::SHORTCUT_UNKNOWN_MODIFIER_KEY).to_string())?;
    set_modifier_localized(settings, platform, key, None, &most_common(&hold.actions), l)
}

pub(crate) fn rows_localized(settings: &Settings, platform: Platform, state: &ShortcutPageState, query: &str, l: &Localizer) -> Vec<ShortcutRow> {
    let all = definitions(platform);
    let mut presses: Vec<_> = all.iter().filter(|(d, _)| d.target.is_none()).collect();
    presses.sort_by_key(|(definition, section)| {
        (SHORTCUT_SECTIONS.iter().position(|s| s == section), !definition.id.starts_with("tools."))
    });
    let filtering = filtering(state, query);
    let single = crate::search::ascii_key_query(query.trim());
    let query = crate::search::normalize(query.trim());
    let effective = |id: &str| -> Vec<KeyChord> {
        settings
            .keys(id)
            .into_iter()
            .filter(|chord| chord.available(platform))
            .filter(|chord| {
                state.context.is_none_or(|category| {
                    settings.shortcut_match(chord, platform, Some(category)).is_some_and(|d| d.id == id)
                })
            })
            .collect()
    };
    presses
        .into_iter()
        .map(|(definition, section)| {
            let hold = hold_id(&definition.id);
            let keys = effective(&definition.id);
            let gestures: Vec<String> = triggers(platform)
                .filter(|t| {
                    if t.held {
                        return settings.pen_actions(t.id).values().any(|id| *id == definition.id);
                    }
                    let bound = settings.gesture_binding(t.id);
                    bound == definition.id || hold.as_deref() == Some(bound)
                })
                .map(|t| t.localized_label(l))
                .collect();
            let modified = settings.shortcut_modified(&definition.id);
            let shortcut = [keys.iter().map(|k| k.localized_label(platform, l)).collect::<Vec<_>>().join(" / "), gestures.join(" · ")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let scope = scope_label_localized(&definition.scope, l);
            let subgroup = brush_tool(definition, l).unwrap_or_default();
            let shared_label = definition.label.named_tool().is_some_and(|tool| {
                all.iter().any(|(other, _)| other.id != definition.id && other.label.named_tool() == Some(tool))
            });
            let detail = detail(definition, !filtering, shared_label, l);
            let matched = match &state.key {
                Some(key) => keys.contains(key),
                None if single => keys.iter().any(|k| key_matches(k, &query, platform)),
                None => matches(&query, &format!("{} {} {subgroup} {shortcut} {scope} {detail} {}", section.localized_label(l), definition.label.resolve(l), definition.label.resolve(&Localizer::shared(UiLanguage::English)))),
            };
            let bound = !keys.is_empty() || !gestures.is_empty();
            let visible = if filtering {
                matched
                    && match state.show {
                        ShortcutShow::All => true,
                        ShortcutShow::Assigned => bound,
                        ShortcutShow::Customized => modified,
                    }
                    && (state.context.is_none() || bound)
            } else {
                state.category.as_deref() == Some(section.id())
            };
            ShortcutRow {
                visible,
                modified,
                shortcut: if shortcut.is_empty() && modified { l.text(MessageId::SHORTCUT_DISABLED).to_string() } else { shortcut },
                bindings: keys.iter().map(|k| k.localized_label_parts(platform, l)).collect(),
                gestures,
                detail,
                subgroup: subgroup.into(),
                scope,
                id: definition.id.clone(),
                label: definition.label.resolve(l),
                group: section.id().to_string(),
            }
        })
        .collect()
}

fn empty(state: &ShortcutPageState, query: &str, platform: Platform, l: &Localizer) -> ShortcutEmpty {
    let (title, description) = if let Some(key) = &state.key {
        (l.text(MessageId::SHORTCUT_NO_SHORTCUT).to_string(), shortcut_key_unassigned(l, key.localized_label(platform, l)))
    } else if !query.trim().is_empty() {
        (l.text(MessageId::SHORTCUT_SEARCH_EMPTY_TITLE).to_string(), l.text(MessageId::SHORTCUT_SEARCH_EMPTY_HELP).to_string())
    } else if state.show == ShortcutShow::Customized {
        (l.text(MessageId::SHORTCUT_CUSTOM_EMPTY_TITLE).to_string(), l.text(MessageId::SHORTCUT_CUSTOM_EMPTY_HELP).to_string())
    } else {
        let tools = state.context.map_or_else(|| l.text(MessageId::SHORTCUT_THESE).to_string(), |c| tool_phrase(c, l).to_string());
        (l.text(MessageId::SHORTCUT_EMPTY_TITLE).to_string(), shortcut_context_empty(l, tools))
    };
    ShortcutEmpty { title, description }
}

pub(crate) fn trigger_rows(settings: &Settings, platform: Platform, l: &Localizer) -> Vec<TriggerRow> {
    let all = definitions(platform);
    let mut ordered: Vec<_> = triggers(platform).collect();
    ordered.sort_by_key(|trigger| !trigger.held);
    ordered
        .into_iter()
        .map(|trigger| {
            let (action, detail) = if trigger.held {
                actions_summary(&all, &settings.pen_actions(trigger.id), l)
            } else {
                (action_label(&all, settings.gesture_binding(trigger.id), l), String::new())
            };
            TriggerRow {
                id: trigger.id.into(),
                section: l.text(if trigger.held { MessageId::SHORTCUT_PEN_BUTTONS } else { MessageId::SHORTCUT_TOUCH_GESTURES }).to_string(),
                label: trigger.localized_label(l),
                action,
                detail,
                modified: settings.gestures.contains_key(trigger.id) || settings.pen_buttons.contains_key(trigger.id),
            }
        })
        .collect()
}

pub(crate) fn view_localized(
    settings: &Settings,
    platform: Platform,
    state: &ShortcutPageState,
    query: &str,
    rows: &[ShortcutRow], l: &Localizer) -> ShortcutPageView {
    let all = definitions(platform);
    let filtering = filtering(state, query);
    let triggers = trigger_rows(settings, platform, l);
    let sections = |query: &str, action: &dyn Fn(&ShortcutRow, &ShortcutDefinition) -> Option<PickerAction>| {
        let query = crate::search::normalize(query.trim());
        SHORTCUT_SECTIONS
            .iter()
            .filter_map(|section| {
                let actions: Vec<_> = rows
                    .iter()
                    .filter(|row| row.group == section.id())
                    .filter_map(|row| all.iter().find(|(d, _)| d.id == row.id).map(|(d, _)| (row, d)))
                    .filter_map(|(row, d)| action(row, d))
                    .filter(|a| matches(&query, &format!("{} {} {} {}", section.localized_label(l), a.label, a.detail, all.iter().find(|(d, _)| d.id == a.id).map_or_else(String::new, |(d, _)| d.label.resolve(&Localizer::shared(UiLanguage::English))))))
                    .collect();
                (!actions.is_empty()).then(|| PickerSection { title: section.localized_label(l), actions })
            })
            .collect::<Vec<_>>()
    };
    let brush = |row: &ShortcutRow| (!row.subgroup.is_empty()).then(|| shortcut_brush_detail(l, row.subgroup.clone()));
    let modifier_picker = state.modifier_picker.as_ref().zip(state.picker.as_ref()).map(|((key, category), (_, picker_query))| {
        let hold = settings.hold_keys(platform).into_iter().find(|h| h.key == *key);
        let current = hold
            .as_ref()
            .and_then(|h| h.actions.get(&category.unwrap_or(CONTEXTS[0])))
            .cloned()
            .unwrap_or_default();
        let label = key.localized_label(platform, l);
        ActionPickerView {
            trigger: crate::shortcuts::MODIFIER_CAPTURE.into(),
            title: match category {
                Some(category) => shortcut_key_context(l, label.clone(), category.localized_label(l).to_string()),
                None => label.clone(),
            },
            description: shortcut_hold_help(l, label.clone()),
            query: picker_query.clone(),
            modified: false,
            nothing: current.is_empty(),
            sections: sections(picker_query, &|row, d| {
                hold_id(&d.id)?;
                Some(PickerAction {
                    id: d.id.clone(),
                    label: held_label(&all, &d.id, l),
                    detail: brush(row).unwrap_or_default(),
                    selected: d.id == current,
                })
            }),
        }
    });
    let pen_picker = state.pen_picker.as_ref().zip(state.picker.as_ref()).and_then(|((trigger, category), (_, picker_query))| {
        let button = GESTURE_TRIGGERS.iter().find(|t| t.id == *trigger)?;
        let current = settings.pen_actions(trigger).get(&category.unwrap_or(CONTEXTS[0])).cloned().unwrap_or_default();
        Some(ActionPickerView {
            trigger: trigger.clone(),
            title: match category {
                Some(category) => shortcut_key_context(l, button.localized_label(l), category.localized_label(l).to_string()),
                None => button.localized_label(l),
            },
            description: l.text(MessageId::SHORTCUT_PEN_ACTION_HELP).to_string(),
            query: picker_query.clone(),
            modified: false,
            nothing: current.is_empty(),
            sections: sections(picker_query, &|row, d| {
                let keys = settings.shortcut_label_localized(&d.id, platform, l);
                let detail = if hold_id(&d.id).is_some() { Some(l.text(MessageId::SHORTCUT_HELD_MODE).to_string()) } else { (!keys.is_empty()).then_some(keys) };
                Some(PickerAction {
                    id: d.id.clone(),
                    label: target_label(&all, &d.id, l),
                    detail: brush(row).into_iter().chain(detail).collect::<Vec<_>>().join(" · "),
                    selected: d.id == current,
                })
            }),
        })
    });
    let picker = modifier_picker.or(pen_picker).or_else(|| state.picker.as_ref().and_then(|(id, picker_query)| {
        let trigger = GESTURE_TRIGGERS.iter().find(|t| t.id == *id)?;
        let current = settings.gesture_binding(trigger.id);
        Some(ActionPickerView {
            trigger: trigger.id.into(),
            title: trigger.localized_label(l),
            description: if trigger.held {
                l.text(MessageId::SHORTCUT_HELD_ACTION_HELP).to_string()
            } else {
                l.text(MessageId::SHORTCUT_TAP_ACTION_HELP).to_string()
            },
            query: picker_query.clone(),
            modified: settings.gestures.contains_key(trigger.id),
            nothing: current.is_empty(),
            sections: sections(picker_query, &|row, d| {
                let held = trigger.held.then(|| hold_id(&d.id)).flatten();
                let keys = settings.shortcut_label_localized(&d.id, platform, l);
                let mode = if held.is_some() { Some(l.text(MessageId::SHORTCUT_HELD_MODE).to_string()) } else { (!keys.is_empty()).then_some(keys) };
                Some(PickerAction {
                    id: d.id.clone(),
                    label: d.label.resolve(l),
                    detail: brush(row).into_iter().chain(mode).collect::<Vec<_>>().join(" · "),
                    selected: d.id == current || held.as_deref() == Some(current),
                })
            }),
        })
    }));
    let modifiers = modifier_rows_localized(settings, platform, state, query, l);
    let visible = rows.iter().filter(|r| r.visible).count() + modifiers.iter().filter(|m| m.visible).count();
    ShortcutPageView {
        contexts: std::iter::once(ShortcutContextChoice { category: None, label: l.text(MessageId::SHORTCUT_ALL_TOOLS).to_string() })
            .chain(CONTEXTS.into_iter().map(|c| ShortcutContextChoice { category: Some(c), label: shortcut_context_choice(l, c.localized_label(l).to_string()) }))
            .collect(),
        context: state.context,
        shows: ShortcutShow::ALL.into_iter().map(|show| ShortcutShowChoice { show, label: show.localized_label(l) }).collect(),
        show: state.show,
        key: state.key.as_ref().map(|k| k.localized_label(platform, l)),
        filtering,
        category: state.category.clone().filter(|_| !filtering),
        categories: std::iter::once(ShortcutCategoryView { id: MODIFIER_SECTION.into(), label: l.text(MessageId::SHORTCUT_MODIFIER_KEYS).to_string(), count: modifiers.len() })
            .chain(SHORTCUT_SECTIONS.iter().map(|section| ShortcutCategoryView {
                id: section.id().to_string(),
                label: section.localized_label(l),
                count: rows.iter().filter(|r| r.group == section.id()).count(),
            }))
            .filter(|c| c.count > 0 || c.id == MODIFIER_SECTION)
            .collect(),
        empty: ((filtering || state.category.is_some()) && visible == 0).then(|| empty(state, query, platform, l)),
        triggers,
        picker,
        modifiers,
    }
}

pub(crate) fn choose_localized(settings: &mut Settings, platform: Platform, trigger: &str, id: &str, l: &Localizer) -> Result<(), String> {
    let trigger = GESTURE_TRIGGERS.iter().find(|t| t.id == trigger).ok_or_else(|| l.text(MessageId::SHORTCUT_UNKNOWN_GESTURE_OR_PEN_BUTTON).to_string())?;
    let held = trigger.held.then(|| hold_id(id)).flatten();
    let id = held.as_deref().unwrap_or(id);
    if !id.is_empty() {
        let (definition, _) = definitions(platform)
            .into_iter()
            .find(|(d, _)| d.id == id)
            .ok_or_else(|| l.text(MessageId::SHORTCUT_UNKNOWN_ACTION).to_string())?;
        if definition.action.held() && !trigger.held {
            return Err(shortcut_cannot_hold(l, trigger.localized_label(l)));
        }
    }
    if id == settings.gesture_default(trigger.id) {
        settings.gestures.remove(trigger.id);
    } else {
        settings.gestures.insert(trigger.id.into(), id.into());
    }
    Ok(())
}







#[cfg(test)]
pub(crate) fn set_pen_button(settings: &mut Settings, platform: Platform, trigger: &str, category: Option<ToolCategory>, action: &str) -> Result<(), String> { set_pen_button_localized(settings, platform, trigger, category, action, &Localizer::shared(UiLanguage::English)) }





#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_cjk_character_searches_action_names() {
        let settings = Settings::default();
        let localizer = Localizer::shared(UiLanguage::Japanese);
        let rows = rows_localized(&settings, Platform::Gtk, &ShortcutPageState::default(), "選", &localizer);
        assert!(rows.iter().any(|row| row.id == CommandId::SelectAll.shortcut_id() && row.visible));
    }

    #[test]
    fn normalized_names_and_ascii_key_queries_have_distinct_routes() {
        let settings = Settings::default();
        let localizer = Localizer::shared(UiLanguage::English);
        let visible = |query| rows_localized(&settings, Platform::Gtk, &ShortcutPageState::default(), query, &localizer)
            .into_iter().any(|row| row.id == CommandId::Undo.shortcut_id() && row.visible);
        assert!(visible("Ｕｎｄｏ"));
        assert!(visible("Ｕ"));
        assert!(!visible("u"));
        assert!(matches(&crate::search::normalize("각"), "각"));
        assert!(matches(&crate::search::normalize("é"), "e\u{301}"));
    }

    #[test]
    fn section_identity_and_saved_settings_survive_localized_projection() {
        let settings = Settings::default();
        let saved = serde_json::to_string(&settings).unwrap();
        let localizer = Localizer::shared(UiLanguage::Japanese);
        let state = ShortcutPageState { category: Some(ShortcutSection::Tools.id().into()), ..Default::default() };
        let rows = rows_localized(&settings, Platform::Gtk, &state, "", &localizer);
        assert!(rows.iter().filter(|row| row.visible).all(|row| row.group == ShortcutSection::Tools.id()));
        assert!(rows.iter().any(|row| row.visible));
        let page = view_localized(&settings, Platform::Gtk, &state, "", &rows, &localizer);
        let tools = page.categories.iter().find(|category| category.id == ShortcutSection::Tools.id()).unwrap();
        assert_eq!(tools.id, "Tools");
        assert_eq!(tools.label, "ツール");
        assert_eq!(serde_json::to_string(&settings).unwrap(), saved);
    }

    #[test]
    fn held_base_labels_follow_semantics_without_english_suffixes() {
        let localizer = Localizer::shared(UiLanguage::Japanese);
        let all = definitions(Platform::Gtk);
        let held = all.iter().find(|(definition, _)| definition.id == "hold.eyedropper").unwrap();
        assert_eq!(held.0.target.as_deref(), Some("command.Eyedropper"));
        assert_eq!(held.0.label.resolve(&localizer), "押している間は色を採取");
        assert_eq!(held_label(&all, "command.Eyedropper", &localizer), "色を採取");
        assert_eq!(target_label(&all, "command.Eyedropper", &localizer), "色を採取");
    }

    #[test]
    fn modifier_search_keeps_canonical_actions_in_every_context() {
        let settings = Settings::default();
        let localizer = Localizer::shared(UiLanguage::Japanese);
        for context in [None, Some(ToolCategory::Drawing)] {
            let state = ShortcutPageState { context, ..Default::default() };
            for query in ["Sample color", "色を採取"] {
                let rows = modifier_rows_localized(&settings, Platform::Gtk, &state, query, &localizer);
                assert!(rows.iter().any(|row| row.key.key == "alt" && row.visible), "{context:?}: {query}");
            }
        }
    }

    #[test]
    fn shortcut_context_phrases_have_catalog_owned_grammar() {
        let settings = Settings::default();
        let state = ShortcutPageState::default();
        let english = Localizer::shared(UiLanguage::English);
        let page = view_localized(&settings, Platform::Gtk, &state, "", &[], &english);
        let expected = [("Drawing tools", "With drawing tools"), ("Erasing tools", "With erasing tools"), ("Blending tools", "With blending tools"), ("Warping tools", "With warping tools"), ("Retouching tools", "With retouching tools"), ("Selection tools", "With selection tools"), ("Fill and gradient tools", "With fill and gradient tools"), ("Shape and ruler tools", "With shape and ruler tools"), ("Move and transform tools", "With move and transform tools"), ("Color sampling tools", "With color sampling tools"), ("Navigation tools", "With navigation tools")];
        for (category, (choice, summary)) in CONTEXTS.into_iter().zip(expected) {
            assert_eq!(page.contexts.iter().find(|choice| choice.category == Some(category)).unwrap().label, choice);
            let actions = std::collections::BTreeMap::from([(category, "command.Eyedropper".to_owned())]);
            assert_eq!(actions_summary(&definitions(Platform::Gtk), &actions, &english).1, summary);
        }
        let key = KeyChord::new("Alt", crate::Modifiers::default());
        let editor = modifier_editor_localized(&settings, Platform::Gtk, &key, true, &english);
        assert_eq!(editor.actions.iter().find(|action| action.category == Some(ToolCategory::Selection)).unwrap().label, "Selection tools");
        assert_eq!(shortcut_key_context(&english, "Alt".into(), ToolCategory::Selection.localized_label(&english).to_string()), "Alt · Selection tools");
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let page = view_localized(&settings, Platform::Gtk, &state, "", &[], &japanese);
        assert_eq!(page.contexts.iter().find(|choice| choice.category == Some(ToolCategory::Selection)).unwrap().label, "選択ツール");
    }

    #[test]
    fn current_brush_detail_requires_shared_tool_name_identity() {
        let settings = Settings::default();
        for language in [UiLanguage::English, UiLanguage::Japanese] {
            let localizer = Localizer::shared(language);
            let rows = rows_localized(&settings, Platform::Gtk, &ShortcutPageState::default(), "", &localizer);
            for command in CommandId::TOOLS {
                if command.paint_tool().is_none() { continue; }
                let row = rows.iter().find(|row| row.id == command.shortcut_id()).unwrap();
                let shared_name = matches!(command, CommandId::Pencil | CommandId::Eraser | CommandId::Airbrush | CommandId::Clone | CommandId::Heal | CommandId::SpotHeal);
                assert_eq!(!row.detail.is_empty(), shared_name, "{command:?} {language:?}");
            }
        }
    }

}
