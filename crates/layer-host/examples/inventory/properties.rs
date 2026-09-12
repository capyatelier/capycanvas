//! Enumerate every shipped filter's real property controls.
//! Shared action/history observations do not establish native widget or pixels.
use super::*;

fn dispatch(host: &mut NativeHost, action: Value) -> Result<(), String> {
    host.dispatch(serde_json::from_value(action).map_err(|e| e.to_string())?)
}

fn values(host: &NativeHost) -> Value {
    json!(
        host.session
            .state()
            .layer_properties
            .controls
            .iter()
            .map(|c| (&c.key, &c.value))
            .collect::<std::collections::BTreeMap<_, _>>()
    )
}

fn lock_action(value: &Value) -> Option<Value> {
    match value {
        Value::Object(object) => {
            if value["enabled"] == true && value["action"]["action"]["op"] == "lock" {
                return Some(value["action"].clone());
            }
            object.values().find_map(lock_action)
        }
        Value::Array(items) => items.iter().find_map(lock_action),
        _ => None,
    }
}

fn changed_value(control: &Value) -> Result<Value, String> {
    let value = &control["value"]["value"];
    Ok(match control["kind"]["kind"].as_str().unwrap() {
        "number" => {
            let numeric = &control["kind"]["numeric"];
            let current = value.as_f64().unwrap();
            let step = numeric["step"].as_f64().unwrap();
            let min = numeric["min"].as_f64().unwrap();
            let max = numeric["max"].as_f64().unwrap();
            json!(if current + step <= max {
                current + step
            } else {
                (current - step).max(min)
            })
        }
        "toggle" => json!(!value.as_bool().unwrap()),
        "choice" => json!(
            (value.as_u64().unwrap() + 1)
                % control["kind"]["options"].as_array().unwrap().len() as u64
        ),
        "color" => {
            let mut color = value.as_array().unwrap().clone();
            color[0] = json!(if color[0].as_f64().unwrap() < 0.5 {
                0.75
            } else {
                0.25
            });
            json!(color)
        }
        "curve" => json!([[0., 0.], [0.5, 0.75], [1., 1.]]),
        "gradient" => json!([
            {"position":0., "color":[0.,0.,0.,1.]},
            {"position":0.375, "color":[0.75,0.25,0.5,1.]},
            {"position":1., "color":[1.,1.,1.,1.]}
        ]),
        kind => return Err(format!("No inventory edit for property kind {kind}")),
    })
}

fn round_trip(host: &mut NativeHost, control: &Value) -> Result<Value, String> {
    let layer = host.session.state().layer_properties.layer.unwrap();
    let key = control["key"].as_str().unwrap();
    let action = json!({"type":"effect", "action":{"op":"set", "layer":layer, "key":key,
        "value":{"kind":control["kind"]["kind"], "value":changed_value(control)?}}});
    let before = values(host);
    dispatch(host, action.clone())?;
    let edited = values(host);
    if before[key] == edited[key] {
        return Err("The proposed edit did not change a value".into());
    }
    let mut expected = before.clone();
    expected[key] = edited[key].clone();
    if edited != expected {
        return Err("The edit changed an unrelated property".into());
    }
    host.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    })?;
    if values(host) != before {
        return Err("Undo did not restore every property".into());
    }
    host.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    })?;
    if values(host) != edited {
        return Err("Redo did not restore every property".into());
    }
    dispatch(
        host,
        json!({"type":"effect", "action":{"op":"reset", "layer":layer, "key":key}}),
    )?;
    let reset = values(host);
    if reset[key] != control["default"] {
        return Err("Reset did not restore the shared default".into());
    }
    Ok(
        json!({"action":action, "before":before[key], "edited":edited[key], "reset":reset[key],
        "undo_restored_all_properties":true, "redo_restored_all_properties":true}),
    )
}

fn capture(mut host: NativeHost, name: &str, setup: Vec<Value>) -> Value {
    let error = setup
        .iter()
        .find_map(|action| dispatch(&mut host, action.clone()).err());
    let initial = json!(host.session.state().layer_properties);
    let controls = initial["controls"].as_array().unwrap();
    let edits = controls
        .iter()
        .map(|control| {
            let result = round_trip(&mut host, control);
            json!({"key":control["key"], "result":result.as_ref().ok(), "error":result.err()})
        })
        .collect::<Vec<_>>();
    let layer = host.session.state().layer_properties.layer.unwrap();
    let menu = json!(host.session.layer_menu(layer, false).unwrap());
    // Paper has no lock action; retain that capability instead of inventing one.
    let lock = lock_action(&menu);
    let lock_error = lock
        .as_ref()
        .and_then(|action| dispatch(&mut host, action.clone()).err());
    let locked = lock
        .as_ref()
        .map(|_| json!(host.session.state().layer_properties));
    let locked_menu = lock
        .as_ref()
        .map(|_| json!(host.session.layer_menu(layer, false).unwrap()));
    let locked_edit = lock.as_ref().and_then(|_| edits.first()).map(|edit| {
        let action = edit["result"]["action"].clone();
        let before = values(&host);
        let error = dispatch(&mut host, action.clone()).err();
        json!({"action":action,"error":error,"values_unchanged":values(&host) == before})
    });
    json!({"name":name, "setup":setup, "error":error, "properties":initial, "edits":edits,
        "menu":menu, "lock_action":lock, "lock_error":lock_error, "locked_properties":locked,
        "locked_menu":locked_menu, "locked_edit":locked_edit})
}

pub(super) fn inventory(platform: Platform) -> Vec<Value> {
    let host = apple_host(platform);
    let paper = host
        .session
        .engine()
        .document()
        .layers
        .iter()
        .find(|l| l.kind == layer_core::LayerKind::Background)
        .unwrap()
        .id
        .0;
    let adjustments = host.session.state().adjustments.clone();
    let mut result = vec![capture(host, "paint", vec![])];
    result.push(capture(
        apple_host(platform),
        "paper",
        vec![json!({"type":"layer", "action":{"op":"select", "id":paper, "mask":false}})],
    ));
    result.push(capture(
        apple_host(platform),
        "group",
        vec![json!({"type":"layer", "action":{"op":"new", "group":true, "clipped":false}})],
    ));
    for filter in adjustments {
        result.push(capture(
            apple_host(platform),
            &format!("filter:{}", filter.id),
            vec![json!(filter.action)],
        ));
    }
    result
}
