use crate::*;
use layer_ui::{LayoutHistory, Platform, WorkspaceCapture, WorkspacePreset, WorkspaceWorkingState};

#[test]
fn phone_defaults_follow_owner_and_preserve_history_and_empty() {
    for (id, preset) in DEFAULT_WORKSPACES
        .into_iter()
        .chain([("custom:Photo", WorkspacePreset::Painter)])
    {
        let layout = WorkspacePreset::Painter.layout(Platform::Gtk);
        let mut entity = Entity::workspace(
            "Photo",
            WorkspaceCapture {
                history: LayoutHistory::new(&layout),
                working: WorkspaceWorkingState::default(),
            },
            layout,
            10,
        );
        entity.id = id.into();
        let ItemContent::Workspace { history, baseline } = &mut entity.content else {
            unreachable!()
        };
        baseline.phone = None;
        let mut older = history.revisions[&history.current].clone();
        older.id = "older".into();
        older.layout.phone = Some(Vec::new());
        history.revisions.insert(older.id.clone(), older);
        history
            .revisions
            .get_mut(&history.current)
            .unwrap()
            .layout
            .phone = None;
        let before = history.clone();
        let mut saved = serde_json::to_value(&entity).unwrap();
        saved["content"]["baseline"]
            .as_object_mut()
            .unwrap()
            .remove("phone");
        saved["content"]["history"]["revisions"][&before.current]["layout"]
            .as_object_mut()
            .unwrap()
            .remove("phone");
        entity = serde_json::from_value(saved).unwrap();
        entity.resolve_phone().unwrap();
        entity.validate().unwrap();
        let ItemContent::Workspace { history, baseline } = &entity.content else {
            unreachable!()
        };
        let controls = |layout: &layer_ui::DockLayout| {
            layout
                .phone
                .as_ref()
                .unwrap()
                .iter()
                .map(|t| t.control)
                .collect::<Vec<_>>()
        };
        assert_eq!(controls(baseline), preset.phone_controls());
        assert_eq!(controls(history.layout()), preset.phone_controls());
        assert!(history.revisions["older"]
            .layout
            .phone
            .as_ref()
            .unwrap()
            .is_empty());
        assert_eq!(
            (
                history.current.clone(),
                history.generation,
                history.undo.clone(),
                history.redo.clone()
            ),
            (before.current, before.generation, before.undo, before.redo)
        );
        let encoded = serde_json::to_string(&entity).unwrap();
        let mut restored: Entity = serde_json::from_str(&encoded).unwrap();
        restored.resolve_phone().unwrap();
        assert_eq!(entity, restored);
    }
}

#[cfg(feature = "native")]
fn phone_manager() -> (std::path::PathBuf, WorkspaceManager<StoreWorker>) {
    let directory = crate::test_support::temp_dir("phone-history");
    let manager = WorkspaceManager::new(StoreWorker::shared(&directory).unwrap(), Platform::Gtk);
    let incoming = pollster::block_on(manager.initialize(1_000)).unwrap();
    manager.activate(incoming);
    (directory, manager)
}

#[test]
#[cfg(feature = "native")]
fn new_workspace_copies_the_authored_phone_toolbar_including_explicit_empty() {
    for empty in [false, true] {
        let (directory, manager) = phone_manager();
        pollster::block_on(async {
            let mut capture = manager.current().unwrap().capture().unwrap();
            let mut layout = capture.history.layout().clone();
            if empty { layout.phone = Some(Vec::new()); }
            else {
                let first = layout.phone.as_ref().unwrap()[0].id;
                layout.remove_phone_tool(first).unwrap();
                layout.insert_phone_tools(None, &[layer_ui::ToolbarControl::Divider, layer_ui::ToolbarControl::ColorPicker]).unwrap();
                layout.phone.as_mut().unwrap()[0].slider_style.expand = true;
            }
            capture.history.append(&layout, layer_ui::LayoutChange::Automatic);
            manager.observe(capture, 2_000);
            let created = manager.create_workspace("Copied phone toolbar", 3_000).await.unwrap();
            let saved = manager.load(&created.entity.id).await.unwrap();
            let ItemContent::Workspace { baseline, history } = &saved.entity.content else { unreachable!() };
            assert_eq!(baseline.phone, layout.phone);
            assert_eq!(history.layout().phone, layout.phone);
            assert!(history.undo.is_empty() && history.redo.is_empty());
            manager.close().await.unwrap();
        });
        drop(manager);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
#[cfg(feature = "native")]
fn phone_and_desktop_restore_each_record_one_exact_undo_redo_step() {
    for phone in [false, true] {
        let (directory, manager) = phone_manager();
        pollster::block_on(async {
            let current = manager.current().unwrap();
            let id = current.id.clone();
            let starting = current.starting_layout(Platform::Gtk, false).unwrap();
            let mut capture = current.capture().unwrap();
            let mut layout = capture.history.layout().clone();
            layout.header.size = layer_ui::HeaderSize::Large;
            layout.bands[0].extent += 19.;
            let first = layout.phone.as_ref().unwrap()[0].id;
            layout.remove_phone_tool(first).unwrap();
            capture.history.append(&layout, layer_ui::LayoutChange::Automatic);
            let undo_count = capture.history.undo.len();
            manager.observe(capture, 2_000);
            let expected = layout.starting_layout_candidate(&starting, phone).unwrap();
            let restored = manager.change_layout_for_scope(&id, None, 3_000, phone).await.unwrap();
            let mut history = restored.entity.capture().unwrap().history;
            assert_eq!(history.layout(), &layer_ui::durable_layout(&expected));
            assert_eq!(history.undo.len(), undo_count + 1);
            assert!(history.redo.is_empty());
            assert!(history.undo());
            assert_eq!(history.layout(), &layer_ui::durable_layout(&layout));
            assert_eq!(history.undo.len(), undo_count);
            assert_eq!(history.redo.len(), 1);
            assert!(history.redo());
            assert_eq!(history.layout(), &layer_ui::durable_layout(&expected));
            assert!(history.redo.is_empty());
            manager.close().await.unwrap();
        });
        drop(manager);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
