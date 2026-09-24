use super::*;
use std::collections::BTreeMap;

#[cfg(test)]
#[test]
fn toolbar_components_upgrade_only_untouched_supported_defaults() {
    use layer_ui::{LayoutHistory, Panel, ToolbarControl, WorkspacePreset};
    for (index, preset, platform) in [
        (0, WorkspacePreset::Painter),
        (2, WorkspacePreset::Photographer),
    ].into_iter().flat_map(|(i, p)| [Platform::Gtk, Platform::Web, Platform::Android].map(|platform| (i, p, platform))) {
        let mut previous = vec![preset.legacy_toolbar_components_layout(platform)];
        previous.push(preset.legacy_bottom_brush_controls_layout(platform));
        if preset == WorkspacePreset::Photographer {
            previous.push(preset.legacy_photo_flip_layout(platform));
        }
        if preset == WorkspacePreset::Painter {
            previous.push(WorkspacePreset::legacy_brush_controls_layout(platform));
        }
        for old in previous {
            let working = preset.working_state();
            let mut entity = Entity::workspace(
                preset.name(),
                WorkspaceCapture {
                    history: LayoutHistory::new(&old),
                    working: working.clone(),
                },
                old.clone(),
                None,
                1,
            );
            entity.id = DEFAULT_WORKSPACES[index].0.into();
            entity.metadata.builtin = true;
            let update = |e: &Entity, p| {
                if index == 0 {
                    updated_painter_default(e, p)
                } else {
                    updated_photographer_default(e, p)
                }
            };
            let updated = update(&entity, platform).unwrap();
            let ItemContent::Workspace {
                baseline, history, ..
            } = &updated
            else {
                panic!()
            };
            assert_eq!(**baseline, preset.layout(platform));
            assert_eq!(history.layout(), baseline.as_ref());
            assert_eq!(entity.working.as_ref(), Some(&working));
            entity.content = updated;
            assert!(update(&entity, platform).is_none());
            let mut customized = old;
            customized
                .insert_tools(Panel::Commands, None, &[ToolbarControl::Color])
                .unwrap();
            if let ItemContent::Workspace {
                baseline, history, ..
            } = &mut entity.content
            {
                **baseline = customized.clone();
                *history = LayoutHistory::new(&customized);
            }
            assert!(
                update(&entity, platform).is_none(),
                "customized baseline is retained"
            );
        }
    }
}

/// Migrate only the untouched shipped Painter, while holding its lease. An
/// edited history (even after Undo), a custom baseline or a copy is never reset.
pub(super) fn updated_painter_default(entity: &Entity, platform: Platform) -> Option<ItemContent> {
    if !layer_ui::CommandId::CustomizeWorkspaceUi.available_on(platform)
        || entity.id != DEFAULT_WORKSPACES[0].0
        || !entity.metadata.builtin
    {
        return None;
    }
    let ItemContent::Workspace {
        history, baseline, ..
    } = &entity.content
    else {
        return None;
    };
    let layout = layer_ui::WorkspacePreset::Painter.layout(platform);
    let previous_bottom_controls = layer_ui::WorkspacePreset::Painter.legacy_bottom_brush_controls_layout(platform);
    let previous_brush_controls = layer_ui::WorkspacePreset::legacy_brush_controls_layout(platform);
    let previous_components = layer_ui::WorkspacePreset::Painter.legacy_toolbar_components_layout(platform);
    let previous_selection = layer_ui::WorkspacePreset::Painter.legacy_selection_layout(platform);
    let previous = layer_ui::WorkspacePreset::legacy_painter_layout(platform);
    let previous_paint_drawer = layer_ui::WorkspacePreset::legacy_painter_paint_drawer_layout(platform);
    let mut previous_native_settings = previous.clone();
    previous_native_settings.header.add(
        layer_ui::HeaderZone::Right, None, &[layer_ui::HeaderItem::Settings],
    ).ok()?;
    let portable = layer_ui::WorkspacePreset::legacy_painter_layout(Platform::Web);
    let mut previous_header = previous_paint_drawer.clone();
    previous_header.header.add(
        layer_ui::HeaderZone::Right, None, &[layer_ui::HeaderItem::Settings],
    ).ok()?;
    let mut previous_with_settings = portable.clone();
    previous_with_settings.header = layer_ui::WorkspacePreset::legacy_painter_paint_drawer_layout(Platform::Gtk).header;
    previous_with_settings.header.add(
        layer_ui::HeaderZone::Right, None, &[layer_ui::HeaderItem::Settings],
    ).ok()?;
    let settings = previous_with_settings.header.zones[2].last()?.id;
    previous_with_settings.header.add(
        layer_ui::HeaderZone::Right, Some(settings), &[layer_ui::HeaderItem::Fullscreen],
    ).ok()?;
    if baseline.as_ref() == &layout
        || history.revisions.len() != 1
        || history.layout() != baseline.as_ref()
        || (baseline.as_ref() != &previous && baseline.as_ref() != &portable
            && baseline.as_ref() != &previous_header && baseline.as_ref() != &previous_with_settings
            && baseline.as_ref() != &previous_native_settings
            && baseline.as_ref() != &previous_paint_drawer
            && baseline.as_ref() != &previous_bottom_controls
            && baseline.as_ref() != &previous_brush_controls
            && baseline.as_ref() != &previous_components
            && baseline.as_ref() != &previous_selection)

    {
        return None;
    }
    let mut content = entity.content.clone();
    if let ItemContent::Workspace {
        history, baseline, ..
    } = &mut content
    {
        history.revisions.values_mut().next()?.layout = layout.clone();
        **baseline = layout;
    }
    Some(content)
}

#[cfg(test)]
#[test]
fn painter_upgrade_preserves_working_values_and_never_resets_edits() {
    for platform in [
        Platform::Gtk,
        Platform::Web,
        Platform::Android,
        Platform::Windows,
        Platform::Ios,
        Platform::Mac,
    ] {
        let old = layer_ui::WorkspacePreset::legacy_painter_layout(platform);
        let mut working = layer_ui::WorkspacePreset::Painter.working_state();
        working.colors.foreground.rgba = [0.2, 0.4, 0.6, 1.];
        let mut entity = Entity::workspace(
            "My Painter",
            WorkspaceCapture {
                history: layer_ui::LayoutHistory::new(&old),
                working: working.clone(),
            },
            old,
            None,
            1,
        );
        entity.id = DEFAULT_WORKSPACES[0].0.into();
        entity.metadata.builtin = true;
        let content = updated_painter_default(&entity, platform).unwrap();
        let mut updated = entity.clone();
        updated.content = content;
        let capture = updated.capture().unwrap();
        assert_eq!(capture.working, working);
        assert_eq!(
            capture.history.layout(),
            &layer_ui::WorkspacePreset::Painter.layout(platform)
        );
        assert!(updated_painter_default(&updated, platform).is_none());
        let mut settings_default = layer_ui::WorkspacePreset::legacy_painter_layout(platform);
        settings_default.header.add(layer_ui::HeaderZone::Right, None, &[layer_ui::HeaderItem::Settings]).unwrap();
        let mut settings_entity = entity.clone();
        settings_entity.content = ItemContent::Workspace {
            history: layer_ui::LayoutHistory::new(&settings_default), baseline: Box::new(settings_default), origin: None,
        };
        assert!(updated_painter_default(&settings_entity, platform).is_some());
        let mut copy = entity.clone();
        copy.id = "user-copy".into();
        assert!(updated_painter_default(&copy, platform).is_none());
        if let ItemContent::Workspace { history, .. } = &mut entity.content {
            let mut layout = history.layout().clone();
            layout.header.size = layer_ui::HeaderSize::Large;
            history.append(&layout, "User customization");
        }
        assert!(updated_painter_default(&entity, platform).is_none());
    }
    // The prior GTK Sketch header ended in Settings. Only an untouched
    // included layout receives the new default; working brush values survive.
    let mut old = layer_ui::WorkspacePreset::legacy_painter_paint_drawer_layout(Platform::Gtk);
    old.header.add(
        layer_ui::HeaderZone::Right, None, &[layer_ui::HeaderItem::Settings],
    ).unwrap();
    let working = layer_ui::WorkspacePreset::Painter.working_state();
    let mut entity = Entity::workspace("Sketch", WorkspaceCapture {
        history: layer_ui::LayoutHistory::new(&old), working: working.clone(),
    }, old.clone(), None, 1);
    entity.id = DEFAULT_WORKSPACES[0].0.into();
    entity.metadata.builtin = true;
    entity.content = ItemContent::Workspace {
        history: layer_ui::LayoutHistory::new(&old),
        baseline: Box::new(old),
        origin: None,
    };
    let mut updated = entity.clone();
    updated.content = updated_painter_default(&entity, Platform::Gtk).unwrap();
    assert_eq!(
        updated.capture().unwrap().history.layout(),
        &layer_ui::WorkspacePreset::Painter.layout(Platform::Gtk)
    );
    assert_eq!(updated.capture().unwrap().working, working);
    assert!(updated_painter_default(&updated, Platform::Gtk).is_none());
    if let ItemContent::Workspace { history, .. } = &mut entity.content {
        let mut layout = history.layout().clone();
        layout.header.size = layer_ui::HeaderSize::Large;
        history.append(&layout, "User customization");
    }
    assert!(updated_painter_default(&entity, Platform::Gtk).is_none());

}

#[cfg(test)]
#[test]
fn brush_drawer_upgrade_only_replaces_untouched_defaults() {
    for platform in [Platform::Gtk, Platform::Android, Platform::Web, Platform::Mac, Platform::Ios, Platform::Windows] {
        let previous = layer_ui::WorkspacePreset::legacy_painter_paint_drawer_layout(platform);
        let mut saved = serde_json::to_value(previous.clone()).unwrap();
        saved["panels"].as_array_mut().unwrap().retain(|panel|
            !["brush_sets", "tools", "sculpt_sets", "filter_types"].iter().any(|id| panel["id"] == *id));
        let old: layer_ui::DockLayout = serde_json::from_value(saved).unwrap();
        assert_eq!(old, previous);
        let mut working = layer_ui::WorkspacePreset::Painter.working_state();
        working.preset = layer_ui::Tool::Pencil.default_preset();
        let mut entity = Entity::workspace("Sketch", WorkspaceCapture {
            history: layer_ui::LayoutHistory::new(&old), working: working.clone(),
        }, old, None, 1);
        entity.id = DEFAULT_WORKSPACES[0].0.into();
        entity.metadata.builtin = true;
        let mut updated = entity.clone();
        updated.content = updated_painter_default(&entity, platform).unwrap();
        assert_eq!(updated.capture().unwrap().working, working);
        assert_eq!(updated.capture().unwrap().history.layout(), &layer_ui::WorkspacePreset::Painter.layout(platform));
        if let ItemContent::Workspace { history, .. } = &mut entity.content {
            let mut layout = history.layout().clone();
            layout.header.size = layer_ui::HeaderSize::Large;
            history.append(&layout, "User customization");
        }
        assert!(updated_painter_default(&entity, platform).is_none());
    }
}

/// Existing, untouched Illustrator workspaces receive the new column default.
/// Customized arrangements and working brush values remain the user's own.
pub(super) fn updated_illustrator_default(
    entity: &Entity,
    platform: Platform,
) -> Option<ItemContent> {
    if entity.id != DEFAULT_WORKSPACES[1].0 || !entity.metadata.builtin {
        return None;
    }
    let ItemContent::Workspace {
        history, baseline, ..
    } = &entity.content
    else {
        return None;
    };
    let layout = layer_ui::WorkspacePreset::Illustrator.layout(platform);
    let previous_collapsed = layer_ui::WorkspacePreset::legacy_illustrator_layout(platform);
    let previous_primary = layer_ui::WorkspacePreset::legacy_illustrator_primary_layout(platform);
    let mut previous = layer_ui::DockLayout::for_platform(platform);
    let without_preferences = previous.clone();
    previous.column_stacks = previous_collapsed.column_stacks.clone();
    if history.revisions.len() != 1 || history.layout() != baseline.as_ref() || baseline.as_ref() == &layout
        || (baseline.as_ref() != &previous && baseline.as_ref() != &without_preferences
            && baseline.as_ref() != &previous_collapsed && baseline.as_ref() != &previous_primary)
    {
        return None;
    }
    let mut content = entity.content.clone();
    if let ItemContent::Workspace {
        history, baseline, ..
    } = &mut content
    {
        history.revisions.values_mut().next()?.layout = layout.clone();
        **baseline = layout;
    }
    Some(content)
}

/// Update only untouched older Photographer arrangements, after their owner
/// has been claimed. Brush edits and renamed workspaces remain intact.
pub(super) fn updated_photographer_default(
    entity: &Entity,
    platform: Platform,
) -> Option<ItemContent> {
    use layer_ui::{Panel, TileStyle, WorkspacePreset};
    if entity.id != DEFAULT_WORKSPACES[2].0 || !entity.metadata.builtin {
        return None;
    }
    let ItemContent::Workspace {
        history, baseline, ..
    } = &entity.content
    else {
        return None;
    };
    if history.revisions.len() != 1 {
        return None;
    }
    let layout = WorkspacePreset::Photographer.layout(platform);
    let previous_components = WorkspacePreset::Photographer.legacy_toolbar_components_layout(platform);
    let previous_inner_bar = WorkspacePreset::Photographer.legacy_bottom_brush_controls_layout(platform);
    let previous_flip = WorkspacePreset::Photographer.legacy_photo_flip_layout(platform);
    let previous_selection = WorkspacePreset::Photographer.legacy_selection_layout(platform);
    let previous_columns = WorkspacePreset::legacy_photographer_layout(platform);
    let previous_primary = WorkspacePreset::legacy_illustrator_primary_layout(platform);
    let mut previous = previous_columns.clone();
    for panel in [Panel::Toolbar, Panel::Commands] {
        previous
            .panels
            .iter_mut()
            .find(|p| p.id == panel)?
            .tile_style = TileStyle::Medium;
    }
    previous.bands[0].extent += TileStyle::Medium.size()[0] - TileStyle::Small.size()[0];
    if baseline.as_ref() == &layout || history.layout() != baseline.as_ref()
        || (baseline.as_ref() != &previous && baseline.as_ref() != &previous_columns && baseline.as_ref() != &previous_primary && baseline.as_ref() != &previous_selection && baseline.as_ref() != &previous_components && baseline.as_ref() != &previous_inner_bar && baseline.as_ref() != &previous_flip) {
        return None;
    }
    let mut content = entity.content.clone();
    if let ItemContent::Workspace {
        history, baseline, ..
    } = &mut content
    {
        history.revisions.values_mut().next()?.layout = layout.clone();
        **baseline = layout;
    }
    Some(content)
}

impl<S: WorkspaceStore> WorkspaceManager<S> {
    /// Migrate immutable legacy inputs atomically, once per source. Distinct
    /// scene sources remain distinct even when their layouts happen to match.
    /// A fallback aliases an identical scene instead of creating another copy.
    /// The host keeps the original files and supplies validated source names.
    pub async fn migrate_legacy(
        &self,
        scenes: &[(String, layer_ui::WorkspaceState)],
        fallback: Option<&(String, layer_ui::WorkspaceState)>,
        now: u64,
    ) -> Result<BTreeMap<String, String>> {
        // Re-read source mappings when another first-start window wins part
        // of our batch. Iteration keeps stack use bounded on native UI workers.
        for _ in 0..8 {
            if let Some(mappings) =
                Box::pin(self.migrate_legacy_once(scenes, fallback, now)).await?
            {
                return Ok(mappings);
            }
        }
        Err(StoreError::new(
            ErrorKind::Conflict,
            "Other windows are still importing saved workspaces. Retry after their import finishes.",
        ))
    }
    async fn migrate_legacy_once(
        &self,
        scenes: &[(String, layer_ui::WorkspaceState)],
        fallback: Option<&(String, layer_ui::WorkspaceState)>,
        now: u64,
    ) -> Result<Option<BTreeMap<String, String>>> {
        let mut mappings = BTreeMap::new();
        let mut mutations = Vec::new();
        let mut imports = Vec::new();
        let mut sources = std::collections::BTreeSet::new();
        for (source, _) in scenes.iter().chain(fallback) {
            if !sources.insert(source) {
                return Err(StoreError::invalid("A legacy source was supplied twice."));
            }
            let StoreResponse::Binding(existing) = self
                .store
                .execute(StoreRequest::LegacyImport {
                    source: source.clone(),
                })
                .await?
            else {
                return Err(StoreError::invalid("Unexpected migration reply."));
            };
            if let Some(id) = existing {
                mappings.insert(source.clone(), id);
            }
        }
        // Validate every unimported input before publishing anything. Previously
        // acknowledged sources are owned by the database, not the legacy file.
        for (source, workspace) in scenes.iter().chain(fallback) {
            if !mappings.contains_key(source) {
                workspace.validate().map_err(StoreError::invalid)?;
            }
        }
        for (source, workspace) in scenes.iter().chain(fallback) {
            if mappings.contains_key(source) {
                continue;
            }
            let identical_scene = fallback
                .is_some_and(|(key, _)| key == source)
                .then(|| scenes.iter().find(|(_, old)| old == workspace))
                .flatten()
                .and_then(|(key, _)| mappings.get(key))
                .cloned();
            let id = if let Some(id) = identical_scene {
                id
            } else {
                let capture = WorkspaceCapture::from_legacy(workspace.clone())
                    .map_err(StoreError::invalid)?;
                let entity = Entity::workspace(
                    "Imported Workspace",
                    capture,
                    workspace.layout.clone(),
                    None,
                    now,
                );
                let id = entity.id.clone();
                mutations.push(Mutation::Create {
                    entity,
                    claim: false,
                    name_policy: NamePolicy::Unique,
                });
                id
            };
            imports.push((source.clone(), id.clone()));
            mappings.insert(source.clone(), id);
        }
        if !imports.is_empty() {
            let known_sources = mappings.len() - imports.len();
            let mut batch = CommitBatch::prepare(self.owner.clone(), mutations)?;
            batch.legacy_imports = imports;
            let operation = batch.operation_id.clone();
            if let Err(error) = self.publish(batch).await {
                // Two native windows may read the legacy files at first start.
                // Use its completed mappings and retire our redundant pending
                // delivery. A partial overlap retries only the unmapped inputs.
                let mut completed = BTreeMap::new();
                for source in mappings.keys() {
                    if let StoreResponse::Binding(Some(id)) = self
                        .store
                        .execute(StoreRequest::LegacyImport {
                            source: source.clone(),
                        })
                        .await?
                    {
                        completed.insert(source.clone(), id);
                    }
                }
                if completed.len() <= known_sources {
                    return Err(error);
                }
                let mut cleanup = CommitBatch::prepare(self.owner.clone(), Vec::new())?;
                cleanup.abandon_operations.push(operation.clone());
                self.publish(cleanup).await?;
                let mut state = self.state.borrow_mut();
                if state
                    .failed_operation
                    .as_ref()
                    .is_some_and(|batch| batch.operation_id == operation)
                {
                    state.failed_operation = None;
                }
                state
                    .older_failed_operations
                    .retain(|batch| batch.operation_id != operation);
                if state.error_operation.as_ref() == Some(&operation) {
                    state.error = None;
                    state.error_operation = None;
                }
                if completed.len() != mappings.len() {
                    return Ok(None);
                }
                mappings = completed;
            }
            self.refresh().await?;
        }
        Ok(Some(mappings))
    }

    /// Local scene/window restoration identity stays outside portable exports.
    pub async fn bind_resume_key(&self, key: &str) -> Result<()> {
        let id = self
            .active_id()
            .ok_or_else(|| StoreError::invalid("No workspace is active."))?;
        let mut batch = CommitBatch::prepare(self.owner.clone(), Vec::new())?;
        batch.bindings.push((key.into(), Some(id)));
        self.publish(batch).await.map(|_| ())
    }
}

#[cfg(test)]
#[test]
fn selection_defaults_upgrade_only_untouched_sketch_and_photo() {
    use layer_ui::{WorkspacePreset, LayoutHistory};
    for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
        for (preset, id, migrate) in [
            (WorkspacePreset::Painter, DEFAULT_WORKSPACES[0].0, updated_painter_default as fn(&Entity,Platform)->Option<ItemContent>),
            (WorkspacePreset::Photographer, DEFAULT_WORKSPACES[2].0, updated_photographer_default),
        ] {
            let old=preset.legacy_selection_layout(platform);
            let mut working=preset.working_state();
            working.selection.tool=layer_ui::SelectionTool::Ellipse;
            working.selection.size=[500.,300.];
            let mut entity=Entity::workspace(preset.name(),WorkspaceCapture {history:LayoutHistory::new(&old),working:working.clone()},old.clone(),None,1);
            entity.id=id.into(); entity.metadata.builtin=true;
            let mut updated=entity.clone(); updated.content=migrate(&entity,platform).unwrap();
            assert_eq!(updated.capture().unwrap().history.layout(),&preset.layout(platform));
            assert_eq!(updated.capture().unwrap().working,working);
            assert!(migrate(&updated,platform).is_none());
            let ItemContent::Workspace {history,..}=&mut entity.content else {panic!()};
            let mut customized=old; customized.header.size=layer_ui::HeaderSize::Large;
            history.append(&customized,"Custom header");
            assert!(migrate(&entity,platform).is_none());
        }
    }
}
