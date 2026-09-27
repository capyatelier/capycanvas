//! Optional field updates for transports that retain the last full UI model.
//! This is independent of the kind of edit: unchanged catalogs, menus and
//! controls never need to be parsed again during ordinary canvas interactions.
use super::NativeHost;
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

type Fields<'a> = BTreeMap<&'a str, &'a RawValue>;

#[derive(Default, Serialize)]
struct Update<'a> {
    model_update: Vec<(Vec<String>, &'a RawValue)>,
    removed: Vec<Vec<String>>,
}

fn difference<'a>(
    previous: &str,
    next: &'a RawValue,
    path: &mut Vec<String>,
    update: &mut Update<'a>,
) -> Result<(), serde_json::Error> {
    if previous == next.get() {
        return Ok(());
    }
    if previous.starts_with('{') && next.get().starts_with('{') {
        // Borrow raw fields: unchanged catalogs are compared as bytes, without
        // allocating or traversing their nested values on the render owner.
        let previous: Fields = serde_json::from_str(previous)?;
        let next: Fields = serde_json::from_str(next.get())?;
        for (&key, &value) in &next {
            path.push(key.to_owned());
            if let Some(old) = previous.get(key) {
                difference(old.get(), value, path, update)?;
            } else {
                update.model_update.push((path.clone(), value));
            }
            path.pop();
        }
        for key in previous.keys().filter(|key| !next.contains_key(*key)) {
            path.push((*key).to_owned());
            update.removed.push(path.clone());
            path.pop();
        }
    } else if previous.starts_with('[') && next.get().starts_with('[') {
        let previous: Vec<&RawValue> = serde_json::from_str(previous)?;
        let items: Vec<&'a RawValue> = serde_json::from_str(next.get())?;
        if previous.len() != items.len() {
            update.model_update.push((path.clone(), next));
            return Ok(());
        }
        for (index, (old, value)) in previous.into_iter().zip(items).enumerate() {
            path.push(index.to_string());
            difference(old.get(), value, path, update)?;
            path.pop();
        }
    } else {
        update.model_update.push((path.clone(), next));
    }
    Ok(())
}

/// The last full model and the byte range of each top-level field, so the next
/// update splits only the new model.
pub(crate) struct ModelBaseline {
    text: String,
    fields: BTreeMap<String, std::ops::Range<usize>>,
}

impl NativeHost {
    /// Same full snapshots and workspace/camera updates as the layout stream;
    /// subsequent full models may use `model_update` (path/value pairs) and
    /// `removed` (paths). Apply them to the last full model before consuming it.
    /// Null is a value, distinct from removal. Paths contain literal object keys,
    /// and a segment under an array is a decimal index. Arrays whose length
    /// changes are replaced whole. The header's primary menu carries only its
    /// title: its sections are the application menus, in order, as submenus.
    pub fn take_model_update_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        let previous = self.last_model_snapshot.take();
        self.model_transport = true;
        let Some(next) = self.layout_update_bytes()? else {
            self.last_model_snapshot = previous;
            return Ok(None);
        };
        let text = String::from_utf8(next).map_err(serde::de::Error::custom)?;
        let fields: Fields = serde_json::from_str(&text)?;
        if !fields.contains_key("state") {
            // Camera/layout messages have their existing host-specific retained
            // model handling. Reestablish a full baseline after those messages.
            return Ok(Some(text.into_bytes()));
        }
        let ranges = fields
            .iter()
            .map(|(&key, value)| {
                let start = value.get().as_ptr() as usize - text.as_ptr() as usize;
                (key.to_owned(), start..start + value.get().len())
            })
            .collect();
        let bytes = match previous {
            Some(previous) => {
                let mut update = Update::default();
                let mut path = Vec::new();
                for (&key, &value) in &fields {
                    path.push(key.to_owned());
                    match previous.fields.get(key) {
                        Some(range) => difference(
                            &previous.text[range.clone()],
                            value,
                            &mut path,
                            &mut update,
                        )?,
                        None => update.model_update.push((path.clone(), value)),
                    }
                    path.pop();
                }
                for key in previous
                    .fields
                    .keys()
                    .filter(|key| !fields.contains_key(key.as_str()))
                {
                    update.removed.push(vec![key.clone()]);
                }
                serde_json::to_vec(&update)?
            }
            None => text.clone().into_bytes(),
        };
        drop(fields);
        self.last_model_snapshot = Some(ModelBaseline {
            text,
            fields: ranges,
        });
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn child<'v>(value: &'v mut Value, key: &Value) -> &'v mut Value {
        let key = key.as_str().unwrap();
        match value {
            Value::Array(items) => &mut items[key.parse::<usize>().unwrap()],
            object => &mut object[key],
        }
    }

    fn apply(value: &mut Value, update: Value) {
        if update.get("model_update").is_none() {
            *value = update;
            return;
        }
        for change in update["model_update"].as_array().unwrap() {
            let mut target = &mut *value;
            for key in change[0].as_array().unwrap() {
                target = child(target, key);
            }
            *target = change[1].clone();
        }
        for path in update["removed"].as_array().unwrap() {
            let path = path.as_array().unwrap();
            let mut target = &mut *value;
            for key in &path[..path.len() - 1] {
                target = child(target, key);
            }
            target
                .as_object_mut()
                .unwrap()
                .remove(path.last().unwrap().as_str().unwrap());
        }
    }

    fn update(previous: &Value, next: &Value) -> Value {
        let mut update = Update::default();
        let old_bytes = serde_json::to_vec(previous).unwrap();
        let new_bytes = serde_json::to_vec(next).unwrap();
        difference(
            std::str::from_utf8(&old_bytes).unwrap(),
            serde_json::from_slice(&new_bytes).unwrap(),
            &mut Vec::new(),
            &mut update,
        )
        .unwrap();
        serde_json::to_value(update).unwrap()
    }

    #[test]
    fn model_updates_preserve_null_removal_arrays_and_literal_keys() {
        let mut previous =
            json!({"x/y": {"~key": null, "removed": 3}, "items": [1,2], "keep": [3]});
        let next = json!({"x/y": {"~key": 4, "new": null}, "items": [2], "keep": [3]});
        let patch = update(&previous, &next);
        apply(&mut previous, patch);
        assert_eq!(previous, next);
    }

    #[test]
    fn model_updates_change_array_elements_in_place() {
        let mut previous = json!({
            "commands": [{"id": "a", "enabled": true}, {"id": "b", "enabled": false, "gone": 1}],
            "rows": [[1, 2], [3, 4]],
            "grown": [1],
        });
        let next = json!({
            "commands": [{"id": "a", "enabled": true}, {"id": "b", "enabled": true}],
            "rows": [[1, 2], [3, 5]],
            "grown": [1, 2],
        });
        let patch = update(&previous, &next);
        assert_eq!(
            patch["model_update"],
            json!([
                [["commands", "1", "enabled"], true],
                [["grown"], [1, 2]],
                [["rows", "1", "1"], 5]
            ])
        );
        assert_eq!(patch["removed"], json!([["commands", "1", "gone"]]));
        apply(&mut previous, patch);
        assert_eq!(previous, next);
    }

    #[test]
    fn a_command_availability_change_sends_only_changed_elements() {
        use layer_ui::{CommandId, Platform, UiAction};
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.resize(2200, 1440, 1.75).unwrap();
        let mut retained: Value =
            serde_json::from_slice(&host.take_model_update_bytes().unwrap().unwrap()).unwrap();
        for command in [CommandId::SelectAll, CommandId::Deselect] {
            host.dispatch(UiAction::Invoke { command }).unwrap();
            let bytes = host.take_model_update_bytes().unwrap().unwrap();
            let patch: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(bytes.len() < 8 * 1024, "{command:?}: {} bytes", bytes.len());
            apply(&mut retained, patch);
            let mut expected = host.snapshot();
            expected["workspace_update"] =
                serde_json::to_value(host.session.workspace_update()).unwrap();
            let expected: Value =
                serde_json::from_slice(&serde_json::to_vec(&expected).unwrap()).unwrap();
            assert_eq!(retained, expected);
        }
    }

    #[test]
    fn the_primary_menu_rebuilds_from_the_application_menus() {
        use layer_ui::{ApplicationMenu, CommandId, Platform, UiAction};
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.resize(2200, 1440, 1.75).unwrap();
        host.dispatch(UiAction::Invoke {
            command: CommandId::SelectAll,
        })
        .unwrap();
        let model: Value =
            serde_json::from_slice(&host.take_model_update_bytes().unwrap().unwrap()).unwrap();
        let compact = &model["header"]["primary_menu"];
        assert_eq!(compact["sections"], json!([]));
        let submenus: Vec<Value> = model["application_menus"]
            .as_array()
            .unwrap()
            .iter()
            .map(|menu| {
                let sections: Vec<Value> = menu["model"]["sections"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|section| !section.as_array().unwrap().is_empty())
                    .cloned()
                    .collect();
                json!({"label": menu["label"], "selected": null, "action": null, "enabled": !sections.is_empty(),
                    "hint": "", "bindings": [], "sections": sections})
            })
            .collect();
        assert_eq!(
            json!({"title": compact["title"], "sections": [submenus]}),
            serde_json::to_value(host.session.application_menu(ApplicationMenu::Primary)).unwrap()
        );
    }

    #[test]
    fn retained_models_match_full_snapshots_and_recover_after_legacy_reads() {
        use layer_ui::{Platform, UiAction};
        for platform in [
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
            Platform::Windows,
        ] {
            let mut host = NativeHost::new(platform).unwrap();
            let read = |host: &mut NativeHost| {
                serde_json::from_slice::<Value>(&host.take_model_update_bytes().unwrap().unwrap())
                    .unwrap()
            };
            let mut retained = read(&mut host);
            for size in [21., 32., 21.] {
                host.dispatch(UiAction::SetBrushSize { value: size })
                    .unwrap();
                let patch = read(&mut host);
                assert!(patch.get("model_update").is_some());
                assert!(
                    serde_json::to_vec(&patch).unwrap().len()
                        < serde_json::to_vec(&retained).unwrap().len() / 2
                );
                apply(&mut retained, patch);
                let mut expected = host.snapshot();
                expected["workspace_update"] =
                    serde_json::to_value(host.session.workspace_update()).unwrap();
                let expected: Value =
                    serde_json::from_slice(&serde_json::to_vec(&expected).unwrap()).unwrap();
                assert_eq!(retained, expected);
                assert!(host.take_model_update_bytes().unwrap().is_none());
            }
            host.take_value();
            host.dispatch(UiAction::SetBrushSize { value: 42. })
                .unwrap();
            assert!(read(&mut host).get("state").is_some());
        }
    }
}
