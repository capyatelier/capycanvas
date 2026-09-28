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
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All actions",
            Self::Assigned => "With shortcuts",
            Self::Customized => "Customized",
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

pub(crate) fn scope_label(scope: &BindingScope) -> String {
    match scope {
        BindingScope::Application => String::new(),
        BindingScope::Canvas => "Canvas".into(),
        BindingScope::Tools { categories } => categories.iter().map(|c| c.label()).collect::<Vec<_>>().join(", "),
    }
}

pub(crate) fn triggers(platform: Platform) -> impl Iterator<Item = &'static GestureTrigger> {
    GESTURE_TRIGGERS
        .iter()
        .filter(move |t| if t.held { platform.pen_buttons() } else { platform.touch_gestures() })
        .filter(move |t| t.id != "pen.button.tertiary" || platform == Platform::Gtk)
}

fn action_label(all: &[(ShortcutDefinition, &str)], id: &str) -> String {
    if id.is_empty() {
        return "Nothing".into();
    }
    all.iter().find(|(d, _)| d.id == id).map_or_else(|| id.to_string(), |(d, _)| d.label.clone())
}

fn matches(query: &str, text: &str) -> bool {
    query.is_empty() || text.to_lowercase().contains(query)
}

fn brush_tool(definition: &ShortcutDefinition) -> Option<&'static str> {
    let crate::shortcuts::ShortcutAction::Action { action } = &definition.action else {
        return None;
    };
    let UiAction::SelectBrush { id } = **action else {
        return None;
    };
    let tool = crate::tools::group(id).tool();
    CommandId::TOOLS.into_iter().find(|c| c.paint_tool() == Some(tool)).map(CommandId::label)
}

fn detail(definition: &ShortcutDefinition, grouped: bool, shared_label: bool) -> String {
    if definition.id.starts_with("tools.") {
        return "Press again to switch between them".into();
    }
    match brush_tool(definition) {
        Some(tool) if !grouped => format!("Brush for the {tool} tool"),
        Some(_) => String::new(),
        None if shared_label && definition.id.starts_with("command.") => "Switch to the tool with its current brush".into(),
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

fn tools_phrase(categories: &[ToolCategory]) -> String {
    let names: Vec<_> = categories.iter().map(|c| c.label().to_lowercase()).collect();
    match names.as_slice() {
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    }
}

fn held_label(all: &[(ShortcutDefinition, &str)], target: &str) -> String {
    if target.is_empty() {
        return "Nothing".into();
    }
    hold_id(target)
        .and_then(|id| all.iter().find(|(d, _)| d.id == id))
        .map_or_else(|| target.to_string(), |(d, _)| d.label.trim_end_matches(" while held").to_string())
}

/// Held actions read as what they do ("Sample color"); others keep their name.
fn target_label(all: &[(ShortcutDefinition, &str)], target: &str) -> String {
    match hold_id(target) {
        Some(_) => held_label(all, target),
        None if target.is_empty() => "Nothing".into(),
        None => all.iter().find(|(d, _)| d.id == target).map_or_else(|| target.to_string(), |(d, _)| d.label.clone()),
    }
}

fn actions_summary(all: &[(ShortcutDefinition, &str)], actions: &std::collections::BTreeMap<ToolCategory, String>) -> (String, String) {
    let mut targets: Vec<&String> = actions.values().collect();
    targets.sort();
    targets.dedup();
    match targets.as_slice() {
        [] => ("Nothing".into(), String::new()),
        [one] if actions.len() == CONTEXTS.len() => (target_label(all, one), String::new()),
        [one] => {
            let tools: Vec<_> = actions.keys().copied().collect();
            (target_label(all, one), format!("With {} tools", tools_phrase(&tools)))
        }
        many => (
            "Depends on the tool".into(),
            many.iter().map(|t| target_label(all, t)).collect::<Vec<_>>().join(" · "),
        ),
    }
}

fn per_tool_rows(
    all: &[(ShortcutDefinition, &str)],
    actions: &std::collections::BTreeMap<ToolCategory, String>,
    per_tool: bool,
) -> (bool, Vec<ModifierKeyAction>) {
    let uniform = CONTEXTS.iter().all(|c| actions.get(c) == actions.get(&CONTEXTS[0]));
    let per_tool = per_tool || !uniform;
    let rows = if per_tool {
        CONTEXTS
            .into_iter()
            .map(|category| ModifierKeyAction {
                category: Some(category),
                label: format!("{} tools", category.label()),
                action: target_label(all, actions.get(&category).map_or("", String::as_str)),
            })
            .collect()
    } else {
        vec![ModifierKeyAction {
            category: None,
            label: "Action".into(),
            action: target_label(all, actions.get(&CONTEXTS[0]).map_or("", String::as_str)),
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

pub(crate) fn pen_editor(settings: &Settings, platform: Platform, trigger: &str, per_tool: bool) -> Option<PenButtonEditor> {
    let definition = GESTURE_TRIGGERS.iter().find(|t| t.id == trigger && t.held)?;
    let all = definitions(platform);
    let (per_tool, actions) = per_tool_rows(&all, &settings.pen_actions(trigger), per_tool);
    Some(PenButtonEditor {
        trigger: trigger.into(),
        label: definition.label.into(),
        per_tool,
        actions,
        modified: settings.pen_buttons.contains_key(trigger) || settings.gestures.contains_key(trigger),
    })
}

pub(crate) fn set_pen_button(
    settings: &mut Settings,
    platform: Platform,
    trigger: &str,
    category: Option<ToolCategory>,
    action: &str,
) -> Result<(), String> {
    if !GESTURE_TRIGGERS.iter().any(|t| t.id == trigger && t.held) {
        return Err("Unknown pen button".into());
    }
    if !action.is_empty() && !definitions(platform).iter().any(|(d, _)| d.id == action && d.target.is_none()) {
        return Err("Unknown action".into());
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

pub(crate) fn unify_pen_button(settings: &mut Settings, platform: Platform, trigger: &str) -> Result<(), String> {
    let common = most_common(&settings.pen_actions(trigger));
    set_pen_button(settings, platform, trigger, None, &common)
}

pub(crate) fn modifier_rows(settings: &Settings, platform: Platform, state: &ShortcutPageState, query: &str) -> Vec<ModifierKeyRow> {
    let all = definitions(platform);
    let defaults = settings.default_hold_keys(platform);
    let filtering = filtering(state, query);
    let query = query.trim().to_lowercase();
    settings
        .hold_keys(platform)
        .into_iter()
        .map(|hold| {
            let (action, detail) = match state.context.and_then(|category| hold.actions.get(&category)) {
                Some(target) => (target_label(&all, target), String::new()),
                None => actions_summary(&all, &hold.actions),
            };
            let label = hold.key.label(platform);
            let modified = !defaults.contains(&hold);
            let bound = match state.context {
                Some(category) => hold.actions.contains_key(&category),
                None => !hold.actions.is_empty(),
            };
            let matched = match &state.key {
                Some(key) => *key == hold.key,
                None if query.chars().count() == 1 => false,
                None => matches(&query, &format!("{MODIFIER_SECTION} {label} {action} {detail}")),
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

pub(crate) fn modifier_editor(settings: &Settings, platform: Platform, key: &KeyChord, per_tool: bool) -> ModifierKeyEditor {
    let all = definitions(platform);
    let hold = settings.hold_keys(platform).into_iter().find(|h| h.key == *key).unwrap_or(HoldKey { key: key.clone(), actions: Default::default() });
    let (per_tool, actions) = per_tool_rows(&all, &hold.actions, per_tool);
    ModifierKeyEditor {
        label: key.label(platform),
        modified: settings.default_hold_keys(platform).iter().any(|d| d.key == hold.key && *d != hold),
        key: key.clone(),
        per_tool,
        actions,
    }
}

pub(crate) fn store_modifiers(settings: &mut Settings, platform: Platform, table: Vec<HoldKey>) {
    settings.hold_keys = (table != settings.default_hold_keys(platform)).then_some(table);
}

pub(crate) fn set_modifier(
    settings: &mut Settings,
    platform: Platform,
    key: &KeyChord,
    category: Option<ToolCategory>,
    action: &str,
) -> Result<(), String> {
    if !action.is_empty() && !hold_id(action).is_some_and(|id| definitions(platform).iter().any(|(d, _)| d.id == id)) {
        return Err("This action can't be used while a key is held".into());
    }
    let mut table = settings.hold_keys(platform);
    let hold = table.iter_mut().find(|h| h.key == *key).ok_or("Unknown modifier key")?;
    apply_action(&mut hold.actions, category, action);
    store_modifiers(settings, platform, table);
    Ok(())
}

pub(crate) fn unify_modifier(settings: &mut Settings, platform: Platform, key: &KeyChord) -> Result<(), String> {
    let hold = settings.hold_keys(platform).into_iter().find(|h| h.key == *key).ok_or("Unknown modifier key")?;
    set_modifier(settings, platform, key, None, &most_common(&hold.actions))
}

pub(crate) fn rows(settings: &Settings, platform: Platform, state: &ShortcutPageState, query: &str) -> Vec<ShortcutRow> {
    let all = definitions(platform);
    let mut presses: Vec<_> = all.iter().filter(|(d, _)| d.target.is_none()).collect();
    presses.sort_by_key(|(definition, section)| {
        (SHORTCUT_SECTIONS.iter().position(|s| s == section), !definition.id.starts_with("tools."))
    });
    let mut labels = std::collections::BTreeMap::<String, usize>::new();
    for (definition, _) in &presses {
        *labels.entry(definition.label.to_lowercase()).or_default() += 1;
    }
    let filtering = filtering(state, query);
    let query = query.trim().to_lowercase();
    let single = query.chars().count() == 1;
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
                .map(|t| t.label.to_string())
                .collect();
            let modified = settings.shortcut_modified(&definition.id);
            let shortcut = [keys.iter().map(|k| k.label(platform)).collect::<Vec<_>>().join(" / "), gestures.join(" · ")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let scope = scope_label(&definition.scope);
            let subgroup = brush_tool(definition).unwrap_or_default();
            let shared_label = labels.get(&definition.label.to_lowercase()).is_some_and(|n| *n > 1);
            let detail = detail(definition, !filtering, shared_label);
            let matched = match &state.key {
                Some(key) => keys.contains(key),
                None if single => keys.iter().any(|k| key_matches(k, &query, platform)),
                None => matches(&query, &format!("{section} {} {subgroup} {shortcut} {scope} {detail}", definition.label)),
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
                state.category.as_deref() == Some(section)
            };
            ShortcutRow {
                visible,
                modified,
                shortcut: if shortcut.is_empty() && modified { "Disabled".into() } else { shortcut },
                bindings: keys.iter().map(|k| k.label_parts(platform)).collect(),
                gestures,
                detail,
                subgroup: subgroup.into(),
                scope,
                id: definition.id.clone(),
                label: definition.label.clone(),
                group: section.to_string(),
            }
        })
        .collect()
}

fn empty(state: &ShortcutPageState, query: &str, platform: Platform) -> ShortcutEmpty {
    let (title, description) = if let Some(key) = &state.key {
        ("No Shortcut".to_string(), format!("{} isn't assigned to anything.", key.label(platform)))
    } else if !query.trim().is_empty() {
        ("No Results Found".into(), "Try a different search.".into())
    } else if state.show == ShortcutShow::Customized {
        ("No Customized Shortcuts".into(), "Shortcuts you change are listed here.".into())
    } else {
        let tools = state.context.map_or("these", |c| c.label()).to_lowercase();
        ("No Shortcuts".into(), format!("Nothing is assigned for {tools} tools."))
    };
    ShortcutEmpty { title, description }
}

pub(crate) fn view(
    settings: &Settings,
    platform: Platform,
    state: &ShortcutPageState,
    query: &str,
    rows: &[ShortcutRow],
) -> ShortcutPageView {
    let all = definitions(platform);
    let filtering = filtering(state, query);
    let mut ordered: Vec<_> = triggers(platform).collect();
    ordered.sort_by_key(|trigger| !trigger.held);
    let triggers: Vec<_> = ordered
        .into_iter()
        .map(|trigger| {
            let (action, detail) = if trigger.held {
                actions_summary(&all, &settings.pen_actions(trigger.id))
            } else {
                (action_label(&all, settings.gesture_binding(trigger.id)), String::new())
            };
            TriggerRow {
                id: trigger.id.into(),
                section: if trigger.held { "Pen buttons" } else { "Touch gestures" }.into(),
                label: trigger.label.into(),
                action,
                detail,
                modified: settings.gestures.contains_key(trigger.id) || settings.pen_buttons.contains_key(trigger.id),
            }
        })
        .collect();
    let sections = |query: &str, action: &dyn Fn(&ShortcutRow, &ShortcutDefinition) -> Option<PickerAction>| {
        let query = query.trim().to_lowercase();
        SHORTCUT_SECTIONS
            .iter()
            .filter_map(|section| {
                let actions: Vec<_> = rows
                    .iter()
                    .filter(|row| row.group == *section)
                    .filter_map(|row| all.iter().find(|(d, _)| d.id == row.id).map(|(d, _)| (row, d)))
                    .filter_map(|(row, d)| action(row, d))
                    .filter(|a| matches(&query, &format!("{section} {} {}", a.label, a.detail)))
                    .collect();
                (!actions.is_empty()).then(|| PickerSection { title: section.to_string(), actions })
            })
            .collect::<Vec<_>>()
    };
    let brush = |row: &ShortcutRow| (!row.subgroup.is_empty()).then(|| format!("Brush for the {} tool", row.subgroup));
    let modifier_picker = state.modifier_picker.as_ref().zip(state.picker.as_ref()).map(|((key, category), (_, picker_query))| {
        let hold = settings.hold_keys(platform).into_iter().find(|h| h.key == *key);
        let current = hold
            .as_ref()
            .and_then(|h| h.actions.get(&category.unwrap_or(CONTEXTS[0])))
            .cloned()
            .unwrap_or_default();
        let label = key.label(platform);
        ActionPickerView {
            trigger: crate::shortcuts::MODIFIER_CAPTURE.into(),
            title: match category {
                Some(category) => format!("{label} · {} tools", category.label()),
                None => label.clone(),
            },
            description: format!("Holding {label} uses this until you let go."),
            query: picker_query.clone(),
            modified: false,
            nothing: current.is_empty(),
            sections: sections(picker_query, &|row, d| {
                hold_id(&d.id)?;
                Some(PickerAction {
                    id: d.id.clone(),
                    label: held_label(&all, &d.id),
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
                Some(category) => format!("{} · {} tools", button.label, category.label()),
                None => button.label.into(),
            },
            description: "Tools, brushes and modes last while the button is held. Other actions run once.".into(),
            query: picker_query.clone(),
            modified: false,
            nothing: current.is_empty(),
            sections: sections(picker_query, &|row, d| {
                let keys = settings.shortcut_label(&d.id, platform);
                let detail = if hold_id(&d.id).is_some() { Some("While held".to_string()) } else { (!keys.is_empty()).then_some(keys) };
                Some(PickerAction {
                    id: d.id.clone(),
                    label: target_label(&all, &d.id),
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
            title: trigger.label.into(),
            description: if trigger.held {
                "Tools, brushes and modes last while the button is held.".into()
            } else {
                "Runs the action once for each tap.".into()
            },
            query: picker_query.clone(),
            modified: settings.gestures.contains_key(trigger.id),
            nothing: current.is_empty(),
            sections: sections(picker_query, &|row, d| {
                let held = trigger.held.then(|| hold_id(&d.id)).flatten();
                let keys = settings.shortcut_label(&d.id, platform);
                let mode = if held.is_some() { Some("While held".to_string()) } else { (!keys.is_empty()).then_some(keys) };
                Some(PickerAction {
                    id: d.id.clone(),
                    label: d.label.clone(),
                    detail: brush(row).into_iter().chain(mode).collect::<Vec<_>>().join(" · "),
                    selected: d.id == current || held.as_deref() == Some(current),
                })
            }),
        })
    }));
    let modifiers = modifier_rows(settings, platform, state, query);
    let visible = rows.iter().filter(|r| r.visible).count() + modifiers.iter().filter(|m| m.visible).count();
    ShortcutPageView {
        contexts: std::iter::once(ShortcutContextChoice { category: None, label: "All tools".into() })
            .chain(CONTEXTS.into_iter().map(|c| ShortcutContextChoice { category: Some(c), label: format!("{} tools", c.label()) }))
            .collect(),
        context: state.context,
        shows: ShortcutShow::ALL.into_iter().map(|show| ShortcutShowChoice { show, label: show.label().into() }).collect(),
        show: state.show,
        key: state.key.as_ref().map(|k| k.label(platform)),
        filtering,
        category: state.category.clone().filter(|_| !filtering),
        categories: std::iter::once(ShortcutCategoryView { id: MODIFIER_SECTION.into(), count: modifiers.len() })
            .chain(SHORTCUT_SECTIONS.iter().map(|section| ShortcutCategoryView {
                id: section.to_string(),
                count: rows.iter().filter(|r| r.group == *section).count(),
            }))
            .filter(|c| c.count > 0 || c.id == MODIFIER_SECTION)
            .collect(),
        empty: ((filtering || state.category.is_some()) && visible == 0).then(|| empty(state, query, platform)),
        triggers,
        picker,
        modifiers,
    }
}

pub(crate) fn choose(settings: &mut Settings, platform: Platform, trigger: &str, id: &str) -> Result<(), String> {
    let trigger = GESTURE_TRIGGERS.iter().find(|t| t.id == trigger).ok_or("Unknown gesture or pen button")?;
    let held = trigger.held.then(|| hold_id(id)).flatten();
    let id = held.as_deref().unwrap_or(id);
    if !id.is_empty() {
        let (definition, _) = definitions(platform)
            .into_iter()
            .find(|(d, _)| d.id == id)
            .ok_or("Unknown action")?;
        if definition.action.held() && !trigger.held {
            return Err(format!("{} cannot hold an action", trigger.label));
        }
    }
    if id == settings.gesture_default(trigger.id) {
        settings.gestures.remove(trigger.id);
    } else {
        settings.gestures.insert(trigger.id.into(), id.into());
    }
    Ok(())
}
