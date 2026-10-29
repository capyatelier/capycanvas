//! Optional field updates for transports that retain the last full UI model.
//! This is independent of the kind of edit: unchanged catalogs, menus and
//! controls never need to be parsed again during ordinary canvas interactions.
use super::NativeHost;
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

use crate::snapshot::RawFields as Fields;

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
struct ModelBaseline {
    text: String,
    fields: BTreeMap<String, std::ops::Range<usize>>,
}

/// Field-update encoder for a retained model, including native host extensions.
#[derive(Default)]
pub struct ModelUpdates {
    baseline: Option<ModelBaseline>,
}

impl ModelUpdates {
    /// Full models establish the baseline; layout, camera and other packets
    /// pass through without changing it.
    pub fn publish(&mut self, next: Vec<u8>) -> Result<Vec<u8>, serde_json::Error> {
        let text = String::from_utf8(next).map_err(serde::de::Error::custom)?;
        let fields: Fields = serde_json::from_str(&text)?;
        if !fields.contains_key("state") { return Ok(text.into_bytes()); }
        let ranges = fields.iter().map(|(&key, value)| {
            let start = value.get().as_ptr() as usize - text.as_ptr() as usize;
            (key.to_owned(), start..start + value.get().len())
        }).collect();
        let bytes = match self.baseline.as_ref() {
            Some(previous) if previous.fields.keys().map(String::as_str).eq(fields.keys().copied()) => {
                let mut update = Update::default();
                let mut path = Vec::new();
                for (&key, &value) in &fields {
                    path.push(key.to_owned());
                    difference(&previous.text[previous.fields[key].clone()], value, &mut path, &mut update)?;
                    path.pop();
                }
                serde_json::to_vec(&update)?
            }
            _ => text.clone().into_bytes(),
        };
        drop(fields);
        self.baseline = Some(ModelBaseline { text, fields: ranges });
        Ok(bytes)
    }
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
        self.model_updates.publish(next).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn extended_metadata_and_scope_changes_publish_replayable_field_updates() {
        let mut transport=ModelUpdates::default();
        let mut model=json!({"state":{"revision":1,"histogram":{"status":"","data":null}},
            "layout":{"groups":[{"id":7}]},"windows_workspace":{"busy":false},"windows_recovery":null});
        let mut retained:Value=serde_json::from_slice(&transport.publish(serde_json::to_vec(&model).unwrap()).unwrap()).unwrap();
        for change in 0..5 {
            model["state"]["revision"]=json!(change+2);
            match change {
                0=>model["windows_workspace"]["busy"]=json!(true),
                1=>model["state"]["histogram"]["status"]=json!("Updating"),
                2=>{model["state"]["histogram"]["status"]=json!("Preview");model["state"]["histogram"]["data"]=json!([1,2]);},
                3=>{model["state"]["histogram"]["status"]=json!("Exact");model["state"]["histogram"]["data"]=json!([3,4]);},
                _=>model["windows_recovery"]=json!({"ready":true}),
            }
            let packet:Value=serde_json::from_slice(&transport.publish(serde_json::to_vec(&model).unwrap()).unwrap()).unwrap();
            assert!(packet.get("state").is_none(),"metadata and scopes must not publish a full model: {packet}");
            assert!(packet["model_update"].as_array().unwrap().iter().all(|pair|pair[0][0]!="layout"));
            apply(&mut retained,packet);assert_eq!(retained,model);
            let camera=br#"{"camera":{"zoom":2}}"#.to_vec();
            assert_eq!(transport.publish(camera.clone()).unwrap(),camera);
        }
    }

    #[test]
    fn a_new_model_contains_interleaved_layout_camera_search_and_glass_updates() {
        let mut transport=ModelUpdates::default();
        let mut model=json!({"state":{"revision":1,"camera":{"zoom":1},"command_search":null,"workspace":{"layout":{"width":240}}},
            "layout":{"groups":[{"height":200}]},"windows_glass":{"panel_revision":1}});
        let mut retained:Value=serde_json::from_slice(&transport.publish(serde_json::to_vec(&model).unwrap()).unwrap()).unwrap();
        for packet in [json!({"layout":{"groups":[{"height":300}]},"workspace_layout":{"width":320}}),
            json!({"camera":{"zoom":2}}),json!({"command_search":{"query":"paint"}}),json!({"windows_glass":{"panel_revision":2}})] {
            assert_eq!(serde_json::from_slice::<Value>(&transport.publish(serde_json::to_vec(&packet).unwrap()).unwrap()).unwrap(),packet);
        }
        model["state"]["revision"]=json!(2);
        model["state"]["camera"]["zoom"]=json!(2);
        model["state"]["command_search"]=json!({"query":"paint"});
        model["state"]["workspace"]["layout"]["width"]=json!(320);
        model["layout"]["groups"][0]["height"]=json!(300);
        model["windows_glass"]["panel_revision"]=json!(2);
        let next=transport.publish(serde_json::to_vec(&model).unwrap()).unwrap();
        apply(&mut retained,serde_json::from_slice(&next).unwrap());
        assert_eq!(retained,model);
    }

    type PanelGeometry=std::collections::BTreeMap<u32,(Vec<[i32;7]>,Vec<[i32;7]>)>;
    fn panel_geometry(mut words: &[i32]) -> PanelGeometry {
        let mut result=PanelGeometry::new();
        while !words.is_empty() {
            assert!(words.len()>=5,"truncated vector header");
            let group=words[0] as u32;
            let commands=usize::try_from(words[3]).unwrap();let triangles=usize::try_from(words[4]).unwrap();
            let end=5+(commands+triangles)*7;assert!(words.len()>=end,"truncated vector geometry");
            let decode=|slice: &[i32]|slice.chunks_exact(7).map(|chunk|chunk.try_into().unwrap()).collect();
            assert!(result.insert(group,(decode(&words[5..5+commands*7]),decode(&words[5+commands*7..end]))).is_none());
            words=&words[end..];
        }
        result
    }

    #[test]
    fn density_only_resize_republishes_current_panel_vectors() {
        use layer_ui::{CustomizationAction, Panel, Platform, UiAction};
        let mut host=NativeHost::new(Platform::Android).unwrap();
        host.resize(1800,1100,1.).unwrap();
        let mut workspace=host.session.state().workspace.clone();
        workspace.layout.bands[0].extent+=0.25;
        host.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(workspace)}).unwrap();
        host.dispatch(UiAction::MeasureHeader {height:40.25,items:Vec::new()}).unwrap();
        let group=host.session.state().workspace.layout.panel_group(Panel::Brushes).unwrap();
        host.dispatch(UiAction::Customize {action:CustomizationAction::SetColumnCollapsed {group,collapsed:true}}).unwrap();
        let column=host.session.state().workspace.layout.collapsed_column_for_group(group).unwrap();
        for action in [CustomizationAction::SetColumnDrawers {column,drawers:false},
            CustomizationAction::ToggleColumnDrawer {group,panel:Panel::Brushes}] {
            host.dispatch(UiAction::Customize {action}).unwrap();
        }
        let mut retained=read(&mut host);
        let initial=retained["layout"]["vector_surfaces"].clone();
        assert!(!initial.as_array().unwrap().is_empty());
        for header in initial.as_array().unwrap() {assert_eq!(header.as_object().unwrap().len(),2,"model stream must carry only group and column");}
        let initial_geometry=panel_geometry(&host.panel_vector_data());
        assert!(!initial_geometry.is_empty());
        assert!(host.take_model_update_bytes().unwrap().is_none());
        let logical=host.logical;
        host.resize(3600,2200,2.).unwrap();
        assert_eq!(host.logical,logical);
        let packed=host.panel_vector_data();
        let baseline=host.model_updates.baseline.as_ref().unwrap().text.clone();
        assert_eq!(host.snapshot_bytes().unwrap(),host.snapshot_bytes().unwrap());
        assert!(host.model_transport);
        assert_eq!(host.panel_vector_data(),packed,"a query cannot replace the published geometry cache");
        assert_eq!(host.model_updates.baseline.as_ref().unwrap().text,baseline);
        let packet=host.take_model_update_bytes().unwrap().expect("density change must publish even when logical extent is unchanged");
        apply(&mut retained,serde_json::from_slice(&packet).unwrap());
        let mut fresh=host.session.layout(logical).with_panel_bridges();
        layer_ui::PanelVectorCache::default().resolve(&mut fresh,2.);
        let expected=Value::Array(fresh.vector_surfaces.iter().map(|surface|json!({"group":surface.group,"column":surface.column})).collect());
        assert_eq!(initial,expected,"density keeps the same surface owners");
        assert_eq!(retained["layout"]["vector_surfaces"],expected);
        let expected_geometry: PanelGeometry=fresh.vector_surfaces.iter().map(|surface|
            (surface.group,(surface.commands.clone(),surface.triangles.clone()))).collect();
        let actual_geometry=panel_geometry(&host.panel_vector_data());
        assert_ne!(initial_geometry,actual_geometry,"fractional native bounds must use the new physical grid");
        assert_eq!(actual_geometry,expected_geometry,"bulk geometry must match a fresh rich UI projection independently of generation");
        assert!(serde_json::to_value(&fresh.vector_surfaces[0]).unwrap()["commands"].is_array(),"canonical UI serialization remains rich");
        assert!(host.take_model_update_bytes().unwrap().is_none());
    }

    #[test]
    fn pending_bubble_pickup_publishes_a_native_model_before_release() {
        use layer_ui::{BubbleAction, BubbleGrab, ContactPhase, Platform, UiAction};
        for platform in [Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios] {
            let mut host = NativeHost::new(platform).unwrap();
            host.resize(1600, 1000, 1.).unwrap();
            let mut workspace = host.session.state().workspace.clone();
            workspace.layout.header = serde_json::from_value(json!({
                "size":"medium", "next_id":3, "zones":[[], [
                    {"id":2,"item":{"kind":"tool","control":{"kind":"panel","panel":"layers"}}}
                ], []]
            })).unwrap();
            host.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
            let mut retained = read(&mut host);
            for (phase, position) in [(ContactPhase::Down, [800., 20.]), (ContactPhase::Move, [640., 520.])] {
                host.dispatch(UiAction::Bubble { action: BubbleAction::Drag {
                    grab: BubbleGrab::Header { id: 2 }, phase, position, viewport: [1600., 1000.],
                }}).unwrap();
            }
            let update = host.take_model_update_bytes().unwrap().expect("canvas entry publishes the new native model");
            apply(&mut retained, serde_json::from_slice(&update).unwrap());
            assert_eq!(retained["bubbles"].as_array().unwrap().len(), 1);
            assert!(retained["bubbles"][0]["view"]["card"].is_object());
            host.dispatch(UiAction::Bubble { action: BubbleAction::Drag {
                grab: BubbleGrab::Header { id: 2 }, phase: ContactPhase::Cancel, position: [640., 520.], viewport: [1600., 1000.],
            }}).unwrap();
            apply(&mut retained, read(&mut host));
            assert!(retained["bubbles"].as_array().unwrap().is_empty(), "cancellation retires the published bubble");
        }
    }

    #[test]
    fn outside_bubble_contact_publishes_retained_model_without_painting() {
        use layer_ui::{BubbleAction, BubbleGrab, ChromeEvent, ChromeFacts, ContactPhase,
            CustomizationAction, Platform, UiAction, UiInput};
        for drawer in [false, true] {
            let mut host = NativeHost::new(Platform::Android).unwrap();
            host.resize(1600, 1000, 1.).unwrap();
            let mut workspace = host.session.state().workspace.clone();
            workspace.layout.header = serde_json::from_value(json!({
                "size":"medium", "next_id":4, "zones":[[{"id":1,"item":{"kind":"capy"}}], [
                    {"id":2,"item":{"kind":"tool","control":{"kind":"panel","panel":"layers"}}},
                    {"id":3,"item":{"kind":"tool","control":{"kind":"command","command":"drawing_brush"}}}
                ], []]
            })).unwrap();
            let layers_header = 2;
            let tools_header = 3;
            host.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
            host.dispatch(UiAction::MeasureHeader { height: 40., items: vec![layer_ui::HeaderItemBounds {
                id: tools_header, bounds: layer_ui::Bounds { x: 800., y: 0., width: 40., height: 40. },
            }] }).unwrap();
            for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Up] {
                host.dispatch(UiAction::Bubble { action: BubbleAction::Drag {
                    grab: BubbleGrab::Header { id: layers_header }, phase, position: [640., 520.], viewport: [1600., 1000.],
                }}).unwrap();
            }
            if drawer {
                host.dispatch(UiAction::Customize { action: CustomizationAction::ToggleHeaderDrawer { id: tools_header } }).unwrap();
                assert!(host.session.state().customization.drawer.is_some());
            }
            let mut retained = read(&mut host);
            assert!(retained["bubbles"][0]["view"]["card"].is_object(), "{retained}");
            let layers_before = retained["state"]["layers"].clone();
            assert!(host.take_model_update_bytes().unwrap().is_none());
            let revision = host.session.state().revision;
            let reply = host.input(UiInput::Chrome {
                event: ChromeEvent::Contact { position: [1500., 950.], canvas: true },
                facts: ChromeFacts::default(), viewport: [1600., 1000.],
            }).unwrap();
            assert!(reply.change.revision > revision, "outside contact must advance the published revision; drawer={drawer}");
            assert_ne!(reply.change.regions & layer_ui::regions::BUBBLES, 0);
            assert!(host.session.bubbles()[0].card.is_none());
            let update = host.take_model_update_bytes().unwrap().expect("touch-only dismissal publishes a model update");
            apply(&mut retained, serde_json::from_slice(&update).unwrap());
            assert!(retained["bubbles"][0]["view"]["card"].is_null());
            assert_eq!(retained["state"]["layers"], layers_before, "Chrome contact does not paint or edit layers");
            if drawer { assert!(retained["state"]["customization"]["drawer"].is_null()); }
            assert!(host.take_model_update_bytes().unwrap().is_none());
        }
    }

    fn read(host: &mut NativeHost) -> Value {
        serde_json::from_slice(&host.take_model_update_bytes().unwrap().unwrap()).unwrap()
    }

    #[test]
    fn snapshot_queries_do_not_acknowledge_pending_ui_or_host_updates() {
        use layer_ui::{Platform,UiAction};
        for model_transport in [false,true] {
            let mut host=NativeHost::new(Platform::Android).unwrap();
            let take=|host: &mut NativeHost|if model_transport {
                host.take_model_update_bytes().unwrap()
            } else {host.take_layout_update_bytes().unwrap()};
            let mut retained: Value=serde_json::from_slice(&take(&mut host).unwrap()).unwrap();
            for service_update in [false,true] {
                if service_update {host.error=Some("Surface lost".into());}
                else {host.dispatch(UiAction::SetBrushSize {value:42.}).unwrap();}
                let acknowledged=host.last_snapshot.clone();
                let revisions=(host.last_workspace_model_revision,host.last_workspace_content_revision,host.last_camera_revision);
                let baseline=host.model_updates.baseline.as_ref().map(|b|b.text.clone());
                let bytes=host.snapshot_bytes().unwrap();
                assert_eq!(bytes,host.snapshot_bytes().unwrap());
                assert!(host.last_snapshot==acknowledged);
                assert_eq!((host.last_workspace_model_revision,host.last_workspace_content_revision,host.last_camera_revision),revisions);
                assert_eq!(host.model_transport,model_transport);
                assert_eq!(host.model_updates.baseline.as_ref().map(|b|b.text.clone()),baseline);
                let current: Value=serde_json::from_slice(&bytes).unwrap();
                let packet=take(&mut host).expect("a query cannot consume the subscriber's pending update");
                apply(&mut retained,serde_json::from_slice(&packet).unwrap());
                assert_eq!(retained["state"],current["state"]);
                assert_eq!(retained["error"],current["error"]);
                assert!(take(&mut host).is_none());
            }
        }
    }

    fn expected_model(host: &NativeHost) -> Value {
        let mut expected = host.snapshot();
        expected["workspace_update"] = serde_json::to_value(host.session.workspace_update()).unwrap();
        serde_json::from_slice(&serde_json::to_vec(&expected).unwrap()).unwrap()
    }


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
            let expected = expected_model(&host);
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
    fn language_adoption_replaces_retained_models_without_waking_the_canvas() {
        use layer_ui::{CommandId, Localizer, Platform, UiAction, UiLanguage};
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.dispatch(UiAction::Invoke { command: CommandId::NewDocument }).unwrap();
        let initial = read(&mut host);
        let document = host.session.engine().document().clone();
        let camera = host.session.state().camera.clone();
        let checkpoint = host.session.engine().checkpoint();
        let epoch = host.session.state().document_file.epoch;
        let requests = serde_json::to_value(&host.session.state().requests).unwrap();
        host.dirty = false;
        for (generation, language) in [(1, UiLanguage::Japanese), (2, UiLanguage::Korean)] {
            let localization = Localizer::shared(language);
            assert!(host.set_localization(localization.clone()));
            let published = read(&mut host);
            assert!(published.get("state").is_some());
            assert!(published.get("model_update").is_none());
            assert_ne!(published["document_options"]["unsaved_description"], initial["document_options"]["unsaved_description"]);
            assert_eq!(host.localization_generation(), generation);
            assert_eq!(host.bootstrap_view().active_tag, language.tag());
            assert_eq!(host.query(json!({"type": "catalog"})).unwrap(), json!(layer_ui::ui_catalog_localized(&localization)));
            assert_eq!(host.session.engine().document(), &document);
            assert_eq!(host.session.engine().checkpoint(), checkpoint);
            assert_eq!(host.session.state().camera, camera);
            assert_eq!(host.session.state().document_file.epoch, epoch);
            assert_eq!(serde_json::to_value(&host.session.state().requests).unwrap(), requests);
            assert!(!host.dirty);
            assert!(!host.set_localization(localization));
            assert!(host.take_model_update_bytes().unwrap().is_none());
        }
    }

    #[test]
    fn camera_messages_between_models_keep_the_update_baseline() {
        use layer_ui::{CommandId, Platform, UiAction};
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.resize(2200, 1440, 1.75).unwrap();
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
            let expected = expected_model(&host);
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
        let mut retained = read(&mut host);
        assert_eq!(retained["state"]["notice"], Value::Null);
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
            });
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
        assert_eq!(retained, expected_model(&host));
        let id = retained["state"]["notice"]["id"].as_u64().unwrap();
        host.dispatch(UiAction::Notice { id, accept: false, action: None }).unwrap();
        let patch = read(&mut host);
        assert_eq!(notice_paths(&patch), [json!([["state", "notice"], null])], "{patch}");
        apply(&mut retained, patch);
        assert_eq!(retained, expected_model(&host));
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
                let expected = expected_model(&host);
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
