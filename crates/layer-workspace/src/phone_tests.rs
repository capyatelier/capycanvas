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
