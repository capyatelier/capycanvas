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
        let fields: Fields = serde_json::from_str(next.get())?;
        if previous.len() != fields.len() || previous.keys().any(|key| !fields.contains_key(key)) {
            update.model_update.push((path.clone(), next));
            return Ok(());
        }
        for (&key, &value) in &fields {
            path.push(key.to_owned());
            difference(previous[key].get(), value, path, update)?;
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
    /// subsequent full models may use `model_update` (path/value pairs). Apply
    /// them to the last full model before consuming it. Paths contain literal
    /// object keys, and a segment under an array is a decimal index. Arrays whose
    /// length changes and objects whose keys change are replaced whole, so every
    /// object keeps the key order of a full model. The header omits its primary
    /// menu; query it as `ApplicationMenu::Primary`. Camera, search and workspace
    /// layout messages leave the baseline alone, so updates stay relative to the
    /// last full model, not to those messages.
    pub fn take_model_update_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        self.model_transport = true;
        let Some(next) = self.layout_update_bytes()? else {
            return Ok(None);
        };
        let text = String::from_utf8(next).map_err(serde::de::Error::custom)?;
        let fields: Fields = serde_json::from_str(&text)?;
        if !fields.contains_key("state") {
            return Ok(Some(text.into_bytes()));
        }
        let ranges = fields
            .iter()
            .map(|(&key, value)| {
                let start = value.get().as_ptr() as usize - text.as_ptr() as usize;
                (key.to_owned(), start..start + value.get().len())
            })
            .collect();
        let bytes = match &self.last_model_snapshot {
            Some(previous)
                if previous
                    .fields
                    .keys()
                    .map(String::as_str)
                    .eq(fields.keys().copied()) =>
            {
                let mut update = Update::default();
                let mut path = Vec::new();
                for (&key, &value) in &fields {
                    path.push(key.to_owned());
                    difference(
                        &previous.text[previous.fields[key].clone()],
                        value,
                        &mut path,
                        &mut update,
                    )?;
                    path.pop();
                }
                serde_json::to_vec(&update)?
            }
            _ => text.clone().into_bytes(),
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
            "commands": [{"id": "a", "enabled": false}, {"id": "b", "enabled": false, "gone": 1}],
            "rows": [[1, 2], [3, 4]],
            "grown": [1],
        });
        let next = json!({
            "commands": [{"id": "a", "enabled": true}, {"enabled": false, "id": "b"}],
            "rows": [[1, 2], [3, 5]],
            "grown": [1, 2],
        });
        let patch = update(&previous, &next);
        assert_eq!(
            patch["model_update"],
            json!([
                [["commands", "0", "enabled"], true],
                [["commands", "1"], {"enabled": false, "id": "b"}],
                [["grown"], [1, 2]],
                [["rows", "1", "1"], 5]
            ])
        );
        apply(&mut previous, patch);
        assert_eq!(previous, next);
    }

    #[test]
    fn a_variant_change_replays_in_the_full_model_key_order() {
        let tabs = r#"{"kind":"tabs","id":44,"panels":["navigator"],"active":"navigator"}"#;
        let split = r#"{"kind":"split","axis":"vertical","first":{"kind":"tabs","id":44,"panels":["navigator"],"active":"navigator"},"fraction":0.5,"second":{"kind":"tabs","id":45,"panels":["layers"],"active":"layers"}}"#;
        let previous = format!(r#"{{"root":{tabs},"size":1}}"#);
        let next = format!(r#"{{"root":{split},"size":1}}"#);
        let next: &RawValue = serde_json::from_str(&next).unwrap();
        let mut update = Update::default();
        difference(&previous, next, &mut Vec::new(), &mut update).unwrap();
        assert_eq!(update.model_update.len(), 1);
        assert_eq!(update.model_update[0].0, ["root"]);
        assert_eq!(update.model_update[0].1.get(), split);
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
    fn workspace_read_only_republishes_open_command_availability() {
        let mut host = NativeHost::new(layer_ui::Platform::Android).unwrap();
        let mut retained: Value = serde_json::from_slice(
            &host.take_model_update_bytes().unwrap().unwrap(),
        ).unwrap();
        for (read_only, enabled) in [(true, false), (false, true)] {
            host.session.set_workspace_read_only(read_only);
            let patch: Value = serde_json::from_slice(
                &host.take_model_update_bytes().unwrap().unwrap(),
            ).unwrap();
            apply(&mut retained, patch);
            let command = retained["state"]["commands"].as_array().unwrap().iter()
                .find(|command| command["id"] == "open_document").unwrap();
            assert_eq!(command["enabled"], enabled);
        }
    }

    #[test]
    fn camera_messages_between_models_keep_the_update_baseline() {
        use layer_ui::{CommandId, Platform, UiAction};
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.resize(2200, 1440, 1.75).unwrap();
        let read = |host: &mut NativeHost| {
            serde_json::from_slice::<Value>(&host.take_model_update_bytes().unwrap().unwrap())
                .unwrap()
        };
        let mut retained = read(&mut host);
        for command in [CommandId::SelectAll, CommandId::Deselect] {
            host.scroll([600., 400.], [0., 40.], 1.75, false, false)
                .unwrap();
            let camera = read(&mut host);
            assert!(
                camera.get("state").is_none() && camera.get("camera").is_some(),
                "{camera}"
            );
            host.dispatch(UiAction::Invoke { command }).unwrap();
            let patch = read(&mut host);
            assert!(
                patch.get("model_update").is_some(),
                "{command:?} sent a full model"
            );
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
    fn a_notice_appears_and_clears_as_one_model_field() {
        use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
        use layer_ui::{CommandId, LayerAction, Platform, UiAction, UiInput};
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.resize(2200, 1440, 1.75).unwrap();
        host.dispatch(UiAction::Invoke { command: CommandId::Move }).unwrap();
        host.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: true } }).unwrap();
        let read = |host: &mut NativeHost| {
            serde_json::from_slice::<Value>(&host.take_model_update_bytes().unwrap().unwrap())
                .unwrap()
        };
        let mut retained = read(&mut host);
        assert_eq!(retained["state"]["notice"], Value::Null);
        let expected = |host: &NativeHost| {
            let mut expected = host.snapshot();
            expected["workspace_update"] =
                serde_json::to_value(host.session.workspace_update()).unwrap();
            serde_json::from_slice::<Value>(&serde_json::to_vec(&expected).unwrap()).unwrap()
        };
        let notice_paths = |patch: &Value| {
            patch["model_update"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|change| change[0].as_array().unwrap().iter().any(|key| key == "notice"))
                .cloned()
                .collect::<Vec<_>>()
        };
        host.session
            .pen(PenEvent {
                device_id: 1,
                sequence: 1,
                timestamp_ns: 1,
                view_revision: host.session.state().camera.revision,
                surface_position: layer_core::Point { x: 1100., y: 700. },
                pressure: 1.,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase: PenPhase::Down,
                tool: ToolKind::Mouse,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        host.input(UiInput::CursorLeave).unwrap();
        let patch = read(&mut host);
        let changes = notice_paths(&patch);
        assert_eq!(changes.len(), 1, "{patch}");
        assert_eq!(changes[0][0], json!(["state", "notice"]));
        assert_eq!(changes[0][1]["text"], "The active layer is locked");
        assert_eq!(changes[0][1]["action"], Value::Null);
        assert!(
            !patch["model_update"].as_array().unwrap().iter().any(|c| c[0] == json!(["state", "commands"])),
            "command objects keep their keys"
        );
        apply(&mut retained, patch);
        assert_eq!(retained, expected(&host));
        let id = retained["state"]["notice"]["id"].as_u64().unwrap();
        host.dispatch(UiAction::Notice { id, accept: false }).unwrap();
        let patch = read(&mut host);
        assert_eq!(notice_paths(&patch), [json!([["state", "notice"], null])], "{patch}");
        apply(&mut retained, patch);
        assert_eq!(retained, expected(&host));
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
