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
    previous: &RawValue,
    next: &'a RawValue,
    path: &mut Vec<String>,
    update: &mut Update<'a>,
) -> Result<(), serde_json::Error> {
    if previous.get() == next.get() {
        return Ok(());
    }
    if previous.get().starts_with('{') && next.get().starts_with('{') {
        // Borrow raw fields: unchanged catalogs are compared as bytes, without
        // allocating or traversing their nested values on the render owner.
        let previous: Fields = serde_json::from_str(previous.get())?;
        let next: Fields = serde_json::from_str(next.get())?;
        for (&key, &value) in &next {
            path.push(key.to_owned());
            if let Some(old) = previous.get(key) {
                difference(old, value, path, update)?;
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
    } else {
        // Arrays are atomic. Hosts retain native item identity by their IDs.
        update.model_update.push((path.clone(), next));
    }
    Ok(())
}

impl NativeHost {
    /// Same full snapshots and workspace/camera updates as the layout stream;
    /// subsequent full models may use `model_update` (path/value pairs) and
    /// `removed` (paths). Apply them to the last full model before consuming it.
    /// Null is a value, distinct from removal. Paths contain literal object keys.
    pub fn take_model_update_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        let previous = self.last_model_snapshot.take();
        let Some(next) = self.take_layout_update_bytes()? else {
            self.last_model_snapshot = previous;
            return Ok(None);
        };
        let fields: Fields = serde_json::from_slice(&next)?;
        if !fields.contains_key("state") {
            // Camera/layout messages have their existing host-specific retained
            // model handling. Reestablish a full baseline after those messages.
            return Ok(Some(next));
        }
        let bytes = if let Some(previous) = previous {
            let mut update = Update::default();
            difference(
                serde_json::from_slice(&previous)?,
                serde_json::from_slice(&next)?,
                &mut Vec::new(),
                &mut update,
            )?;
            serde_json::to_vec(&update)?
        } else {
            next.clone()
        };
        self.last_model_snapshot = Some(next);
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn apply(value: &mut Value, update: Value) {
        if update.get("model_update").is_none() {
            *value = update;
            return;
        }
        for change in update["model_update"].as_array().unwrap() {
            let mut target = &mut *value;
            for key in change[0].as_array().unwrap() {
                target = &mut target[key.as_str().unwrap()];
            }
            *target = change[1].clone();
        }
        for path in update["removed"].as_array().unwrap() {
            let path = path.as_array().unwrap();
            let mut target = &mut *value;
            for key in &path[..path.len() - 1] {
                target = &mut target[key.as_str().unwrap()];
            }
            target
                .as_object_mut()
                .unwrap()
                .remove(path.last().unwrap().as_str().unwrap());
        }
    }

    #[test]
    fn model_updates_preserve_null_removal_arrays_and_literal_keys() {
        let mut previous =
            json!({"x/y": {"~key": null, "removed": 3}, "items": [1,2], "keep": [3]});
        let next = json!({"x/y": {"~key": 4, "new": null}, "items": [2], "keep": [3]});
        let mut update = Update::default();
        let old_bytes = serde_json::to_vec(&previous).unwrap();
        let new_bytes = serde_json::to_vec(&next).unwrap();
        difference(
            serde_json::from_slice(&old_bytes).unwrap(),
            serde_json::from_slice(&new_bytes).unwrap(),
            &mut Vec::new(),
            &mut update,
        )
        .unwrap();
        apply(&mut previous, serde_json::to_value(update).unwrap());
        assert_eq!(previous, next);
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
            host.take_snapshot();
            host.dispatch(UiAction::SetBrushSize { value: 42. })
                .unwrap();
            assert!(read(&mut host).get("state").is_some());
        }
    }
}
