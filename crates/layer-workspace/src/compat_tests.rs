use crate::*;
use layer_ui::{EditingState, Localizer, Platform, UiLanguage};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn fixture_root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat") }
fn write_json(directory: &Path, name: &str, value: &impl serde::Serialize) {
    std::fs::write(directory.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[test]
#[ignore]
fn write_compat_fixtures() {
    let directory = PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT"));
    generate(&directory);
}

fn generate(output: &Path) {
    let directory = crate::test_support::temp_dir("compat-generator");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::create_dir_all(output).unwrap();
    let localizer = Localizer::shared(UiLanguage::English);
    let mut editing = EditingState::default();
    for id in layer_ui::brush_ids() {
        editing.tools.remember(id);
        editing.tools.set_override(id, "size", 37., &localizer).unwrap();
    }
    for slot in layer_ui::ToolSlotId::ALL {
        editing.tool_slots.choices.insert(slot, *slot.variants().last().unwrap());
    }
    let action = editing.color_library.prepare_creation(layer_ui::ColorLibraryAction::CreatePalette { name: "Release palette".into() }, &localizer).unwrap();
    editing.color_library.apply(action).unwrap();
    let palette = editing.color_library.palettes.last().unwrap().id;
    let color = serde_json::from_value(json!({"space":"Srgb","rgba":[0.2,0.4,0.6,1.]})).unwrap();
    let action = editing.color_library.prepare_creation(layer_ui::ColorLibraryAction::Store {palette, name: "Release blue".into(), color}, &localizer).unwrap();
    editing.color_library.apply(action).unwrap();
    editing.colors.set_rgba([0.2,0.3,0.4,1.]).unwrap();
    for (index,value) in editing.region_sources.iter_mut().enumerate() { *value = [layer_ui::RegionSource::Visible,layer_ui::RegionSource::Reference,layer_ui::RegionSource::Editing][index % 3]; }
    for (id, value) in &mut editing.region_values { *value = if id == "smoothing" {0.} else if id == "tolerance" {0.4} else if id == "grain_depth" {0.5} else {2.}; }
    editing.validate().unwrap();
    write_json(&directory, "editing.json", &editing);
    let mut options = editing.clone();
    for (index,value) in options.region_sources.iter_mut().enumerate() { *value = if index == 0 {Default::default()} else {layer_ui::RegionSource::Editing}; }
    if let Some(smoothing) = options.region_values.get_mut("smoothing") { *smoothing = 1.; }
    write_json(&directory,"editing-options.json",&options);
    let mut settings = layer_ui::Settings {
        theme: Some(layer_ui::Theme::Dark), pan_speed: 1.5, zoom_speed: 0.75,
        quick_pinch_fit: false, ..Default::default()
    };
    settings.shortcuts.insert("command.Undo".into(), vec![]);
    settings.validate_localized(&localizer).unwrap();
    write_json(&directory, "settings.json", &settings);
    let mut presets = layer_ui::ExportPresets::default();
    presets.save("Release delivery", layer_ui::ExportRecipe::wide_color()).unwrap();
    presets.remember(0, layer_ui::ExportRecipe::wide_color()).unwrap();
    std::fs::write(directory.join("export-presets"), presets.encode().unwrap()).unwrap();
    let mut controls: Vec<_> = layer_ui::CommandId::ALL.into_iter().map(|command| layer_ui::ToolbarControl::Command {command}).collect();
    controls.extend(layer_ui::ToolSlotId::ALL.into_iter().map(|slot| layer_ui::ToolbarControl::ToolSlot {slot}));
    controls.extend(layer_ui::Panel::ALL.into_iter().map(|panel| layer_ui::ToolbarControl::Panel {panel}).filter(|control| control.validate_admission().is_ok()));
    controls.extend(layer_ui::brush_ids().map(|id| layer_ui::ToolbarControl::Brush {id}));
    let definition = ToolbarDefinition {name: "Release toolbar".into(), tiles: controls.into_iter().enumerate().map(|(index,control)| layer_ui::ToolbarTile {id:index as u32+1,control,slider_style:Default::default()}).collect(), tile_style: layer_ui::TileStyle::Large, hide_tab: true};
    definition.validate().unwrap();
    let toolbar = Entity::toolbar(definition, 1000);
    let mut layout = layer_ui::WorkspacePreset::Illustrator.layout(Platform::Gtk);
    layout.canvas_bar = false;
    let mut capture = layer_ui::WorkspaceCapture::from_template(&layout).unwrap();
    capture.working.zen_mode = true;
    let custom = Entity::workspace("Release workspace", capture, layout, 1000);
    std::fs::write(directory.join("workspace.capyworkspace"), export_package(&custom).unwrap()).unwrap();
    std::fs::write(directory.join("toolbar.capytoolbar"), export_package(&toolbar).unwrap()).unwrap();
    let mut entities = vec![custom, toolbar];
    for (id,_) in DEFAULT_WORKSPACES {
        for platform in Platform::ALL {
            for tablet in [false,true] {
                let entity = Entity::included_workspace(id,platform,tablet,1000).unwrap();
                if id == DEFAULT_WORKSPACES[1].0 && tablet == matches!(platform,Platform::Android | Platform::Ios) {
                    write_json(&directory, &format!("{}-{platform:?}-{tablet}.json",id.rsplit(':').next().unwrap()), &entity);
                }
                if platform == Platform::Gtk && !tablet { entities.push(entity); }
            }
        }
    }
    if let ItemContent::Workspace {history,..} = &mut entities[2].content {
        history.revisions.get_mut(&history.current).unwrap().layout.canvas_bar = false;
    }
    write_json(&directory,"entities.json",&entities);
    let mut batch = CommitBatch::prepare(Owner::fresh(), entities.into_iter().map(|entity| Mutation::Create {entity,claim:false,name_policy:NamePolicy::Exact}).collect()).unwrap();
    batch.editing_json = Some(serde_json::to_string(&editing).unwrap());
    let mut native = SqliteStore::open(&directory.join("workspaces.sqlite3")).unwrap();
    native.handle(StoreRequest::Commit {batch:batch.clone()}).unwrap();
    native.handle(StoreRequest::Reopen).unwrap();
    drop(native);
    let mut browser = BrowserDatabase::default();
    browser.prepare_delivery(&batch).unwrap();
    browser.execute(StoreRequest::Commit {batch},1000).unwrap();
    std::fs::write(directory.join("browser.json"), browser.encoded().unwrap()).unwrap();
    for entry in std::fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if name.ends_with(".json") || matches!(name,"export-presets" | "workspace.capyworkspace" | "toolbar.capytoolbar" | "workspaces.sqlite3") {
            std::fs::copy(&path,output.join(name)).unwrap();
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

fn expect(release: &str, name: &str, actual: Value) {
    let path = fixture_root().join("expected").join(release).join(format!("{name}.expected.json"));
    if std::env::var_os("CAPY_BLESS").as_deref() == Some(std::ffi::OsStr::new("1")) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
    }
    let expected: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(actual, expected, "{}", path.display());
}
fn portable(mut entity: Entity) -> Value {
    entity.id.clear();
    entity.metadata.last_used_ms = 0;
    entity.metadata.builtin = false;
    if let ItemContent::Workspace {history,..} = &mut entity.content {
        let ids: std::collections::BTreeMap<_,_> = history.revisions.keys().enumerate().map(|(index,id)| (id.clone(),format!("r{index}"))).collect();
        history.current = ids[&history.current].clone();
        for id in history.undo.iter_mut().chain(&mut history.redo) { *id = ids[id].clone(); }
        history.revisions = std::mem::take(&mut history.revisions).into_iter().map(|(id,mut revision)| {
            revision.id = ids[&id].clone(); (revision.id.clone(),revision)
        }).collect();
        history.generation = 0;
    }
    serde_json::to_value(entity).unwrap()
}

#[test]
fn saved_documents_tolerate_missing_and_unknown_fields() {
    let entities:Vec<Entity>=serde_json::from_slice(&std::fs::read(fixture_root().join("1.0.15/entities.json")).unwrap()).unwrap();
    for entity in entities {
        let original=serde_json::to_value(&entity).unwrap();let mut unknown=original.clone();
        for pointer in ["","/metadata","/content","/working","/content/history","/content/baseline","/content/definition"] {
            if let Some(fields)=unknown.pointer_mut(pointer).and_then(Value::as_object_mut) {fields.insert("future".into(),true.into());}
        }
        if let Some(revisions)=unknown.pointer_mut("/content/history/revisions").and_then(Value::as_object_mut) {
            for revision in revisions.values_mut() {
                revision["future"]=true.into();revision["layout"]["future"]=true.into();
                revision["layout"]["header"]["future"]=true.into();
            }
        }
        if let Some(tiles)=unknown.pointer_mut("/content/definition/tiles").and_then(Value::as_array_mut) {
            for tile in tiles {tile["future"]=true.into();tile["control"]["future"]=true.into();tile["slider_style"]["future"]=true.into();}
        }
        let decoded:Entity=serde_json::from_value(unknown).unwrap();decoded.validate().unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(),original);
        for pointer in ["/id","/metadata/kind","/content/type"] {
            let mut missing=original.clone();let (parent,field)=pointer.rsplit_once('/').unwrap();
            missing.pointer_mut(parent).unwrap().as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<Entity>(missing).is_err(),"{pointer}");
        }
    }
}

#[test]
fn release_fixtures_load_without_loss() {
    for entry in std::fs::read_dir(fixture_root()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() && path.file_name().unwrap() != "expected" {check_release(path.file_name().unwrap().to_str().unwrap());}
    }
}

fn check_release(release: &str) {
    check_inputs(&fixture_root().join(release), |name,value|expect(release,name,value));
}

#[test]
fn generated_compat_fixtures_load_without_expectations() {
    let directory = crate::test_support::temp_dir("generated-compat");
    generate(&directory);
    check_inputs(&directory, |_,_| {});
    std::fs::remove_dir_all(directory).unwrap();
}

fn check_inputs(directory: &Path, check: impl Fn(&str, Value)) {
    let english = Localizer::shared(UiLanguage::English);
    let mut paths: Vec<_> = std::fs::read_dir(directory).unwrap().map(|p|p.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        let name = path.file_name().unwrap().to_str().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let value = match name {
            name if name.starts_with("editing") => serde_json::to_value(EditingState::restore(std::str::from_utf8(&bytes).unwrap(), &english)).unwrap(),
            "settings.json" => serde_json::to_value(layer_ui::Settings::restore(std::str::from_utf8(&bytes).unwrap())).unwrap(),
            "export-presets" => serde_json::to_value(layer_ui::ExportPresets::restore(&bytes)).unwrap(),
            "entities.json" => serde_json::to_value(serde_json::from_slice::<Vec<Entity>>(&bytes).unwrap()).unwrap(),
            "browser.json" | "workspaces.sqlite3" => continue,
            name if name.ends_with(".capyworkspace") => portable(import_package(&bytes,PackageKind::WorkspaceBackup,9000).unwrap()),
            name if name.ends_with(".capytoolbar") => portable(import_package(&bytes,PackageKind::Toolbar,9000).unwrap()),
            _ => {
                let entity: Entity = serde_json::from_slice(&bytes).unwrap();
                entity.validate().unwrap(); serde_json::to_value(entity).unwrap()
            }
        };
        check(name,value);
    }
    let entities: Vec<Entity> = serde_json::from_slice(&std::fs::read(directory.join("entities.json")).unwrap()).unwrap();
    let mut browser = BrowserDatabase::decode(&std::fs::read_to_string(directory.join("browser.json")).unwrap()).unwrap();
    for entity in &entities {
        let StoreResponse::Entity(loaded) = browser.execute(StoreRequest::Load {id:entity.id.clone()},1000).unwrap() else {panic!()};
        assert_eq!(portable(loaded.entity),portable(entity.clone()));
    }
    let temp = crate::test_support::temp_dir("released-store");
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::copy(directory.join("workspaces.sqlite3"), temp.join("workspaces.sqlite3")).unwrap();
    let expected = serde_json::to_value(EditingState::restore(&std::fs::read_to_string(directory.join("editing.json")).unwrap(),&english)).unwrap();
    let StoreResponse::Editing {state:editing,..} = browser.execute(StoreRequest::Editing {language:UiLanguage::English},1000).unwrap() else {panic!()};
    assert_eq!(serde_json::to_value(editing).unwrap(),expected);
    {
        let manager = WorkspaceManager::new(StoreWorker::shared(&temp).unwrap(), Platform::Gtk);
        pollster::block_on(manager.load_editing()).unwrap();
        assert_eq!(serde_json::to_value(manager.editing()).unwrap(), expected);
    }
    for _ in 0..2 {
        let (controller,mut host) = crate::controller_tests::start_native(&temp);
        assert!(controller.view.ready, "{:?}",controller.view.error);
        assert!(!controller.manager.in_memory());
        assert!(host.session.state().notice.is_none());
        for entity in &entities {
            let loaded = pollster::block_on(controller.manager.load(&entity.id)).unwrap();
            assert_eq!(portable(loaded.entity),portable(entity.clone()));
        }
        crate::controller_tests::close_native(controller,&mut host);
    }
    std::fs::remove_dir_all(temp).unwrap();
}
