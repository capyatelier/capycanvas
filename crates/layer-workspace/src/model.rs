use layer_ui::{
    DockLayout, LayoutHistory, PanelConfig, TileStyle, ToolbarTile, WorkspaceCapture,
    WorkspaceWorkingState,
};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 7;
pub const HISTORY_BUDGET_BYTES: u64 = 100 * 1024 * 1024;
pub const OWNER_LEASE_MS: u64 = 30_000;
pub const OWNER_RENEW_MS: u64 = 10_000;
pub const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;
/// Stable identities: included names are system-owned; workspace content is editable.
pub const DEFAULT_WORKSPACES: [(&str, layer_ui::WorkspacePreset); 3] = [
    (
        "builtin:workspace:painter",
        layer_ui::WorkspacePreset::Painter,
    ),
    (
        "builtin:workspace:illustrator",
        layer_ui::WorkspacePreset::Illustrator,
    ),
    (
        "builtin:workspace:photographer",
        layer_ui::WorkspacePreset::Photographer,
    ),
];
pub(crate) fn is_default_item(id: &str) -> bool {
    DEFAULT_WORKSPACES.iter().any(|(key, _)| *key == id)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}
pub fn validate_name(name: &str) -> Result<(), StoreError> {
    if name.trim() != name
        || name.is_empty()
        || name.chars().count() > 100
        || name.chars().any(char::is_control)
    {
        return Err(StoreError::invalid(
            "Enter a name of 1–100 characters without leading or trailing spaces.",
        ));
    }
    Ok(())
}
pub(crate) use layer_ui::counter;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Conflict,
    OwnedElsewhere,
    UnsupportedSchema,
    Unavailable,
    FailedWrite,
    StorageFull,
    InvalidData,
    NotFound,
    NameCollision,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreError {
    pub kind: ErrorKind,
    pub message: String,
}
impl StoreError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidData, message)
    }
    pub fn conflict() -> Self {
        Self::new(
            ErrorKind::Conflict,
            "This item changed in another window. Your changes have been kept.",
        )
    }
}
impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for StoreError {}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::invalid(e.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Workspace,
    Toolbar,
}
impl ItemKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Toolbar => "toolbar",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Toolbar => "Toolbar",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub kind: ItemKind,
    pub name: String,
    pub description: String,
    /// Included workspaces cannot be renamed or deleted; their content is editable.
    pub builtin: bool,
    #[serde(with = "counter")]
    pub created_at_ms: u64,
    #[serde(with = "counter")]
    pub modified_at_ms: u64,
    #[serde(with = "counter")]
    pub last_used_ms: u64,
}
impl Metadata {
    pub fn new(kind: ItemKind, name: &str, now: u64) -> Self {
        Self {
            kind,
            name: name.trim().into(),
            description: String::new(),
            builtin: false,
            created_at_ms: now,
            modified_at_ms: now,
            last_used_ms: now,
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        validate_name(&self.name)?;
        if self.description.len() > 16_384 {
            return Err(StoreError::invalid(
                "Item metadata exceeds supported limits.",
            ));
        }
        Ok(())
    }
    pub fn rename(&mut self, name: &str, description: &str, now: u64) -> Result<(), StoreError> {
        if self.builtin && self.kind == ItemKind::Workspace {
            return Err(StoreError::invalid(
                "Included workspaces cannot be renamed.",
            ));
        }
        validate_name(name.trim())?;
        if description.len() > 16_384 {
            return Err(StoreError::invalid(
                "Item metadata exceeds supported limits.",
            ));
        }
        if name.trim() == self.name && description == self.description {
            return Ok(());
        }
        self.name = name.trim().into();
        self.description = description.into();
        self.modified_at_ms = now;
        self.validate()
    }
}

/// A library toolbar deliberately has no panel/group identity or screen position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolbarDefinition {
    pub name: String,
    pub tiles: Vec<ToolbarTile>,
    pub tile_style: TileStyle,
    pub hide_tab: bool,
}
impl ToolbarDefinition {
    pub fn capture(panel: &PanelConfig) -> Result<Self, StoreError> {
        let layer_ui::PanelContent::Toolbar { name, tiles } = &panel.content else {
            return Err(StoreError::invalid("Choose a toolbar."));
        };
        Ok(Self {
            name: name.clone(),
            tiles: tiles.clone(),
            tile_style: panel.tile_style,
            hide_tab: panel.hide_tab,
        })
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        validate_name(&self.name)?;
        if self.tiles.len() > 4096 {
            return Err(StoreError::invalid("Toolbar has too many controls."));
        }
        if self.tiles.iter().any(|tile| tile.id == u32::MAX) {
            return Err(StoreError::invalid("Toolbar identities are exhausted."));
        }
        PanelConfig {
            id: layer_ui::Panel::Toolbar, hide_tab: self.hide_tab, tile_style: self.tile_style,
            content: layer_ui::PanelContent::Toolbar { name: self.name.clone(), tiles: self.tiles.clone() },
        }.validate().map_err(StoreError::invalid)?;
        let mut ids = std::collections::BTreeSet::new();
        if self.tiles.iter().any(|tile| tile.id == 0 || !ids.insert(tile.id)) {
            return Err(StoreError::invalid("Duplicate or invalid toolbar tile identity"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemContent {
    Workspace {
        history: LayoutHistory,
        baseline: Box<DockLayout>,
    },
    Toolbar {
        definition: ToolbarDefinition,
    },
}
impl ItemContent {
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Workspace { .. } => ItemKind::Workspace,
            Self::Toolbar { .. } => ItemKind::Toolbar,
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        match self {
            Self::Workspace { history, baseline } => {
                history.validate().map_err(StoreError::invalid)?;
                validate_stored_layout(baseline)
            }
            Self::Toolbar { definition } => definition.validate(),
        }
    }
}
fn validate_stored_layout(layout: &DockLayout) -> Result<(), StoreError> {
    layout.validate().map_err(StoreError::invalid)?;
    if &layer_ui::durable_layout(layout) != layout {
        return Err(StoreError::invalid(
            "Stored layouts cannot include measured widget geometry or viewport fitting.",
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub metadata: Metadata,
    pub content: ItemContent,
    pub working: Option<WorkspaceWorkingState>,
}
impl Entity {
    /// The same platform-specific definition seeds and repairs included workspaces.
    pub(crate) fn included_workspace(
        id: &str,
        platform: layer_ui::Platform,
        now: u64,
    ) -> Option<Self> {
        let (_, preset) = DEFAULT_WORKSPACES.iter().find(|(key, _)| *key == id)?;
        let layout = preset.layout(platform);
        let mut entity = Self::workspace(
            preset.name(),
            WorkspaceCapture {
                history: LayoutHistory::new(&layout),
                working: preset.working_state(),
            },
            layout,
            now,
        );
        entity.id = id.into();
        entity.metadata.builtin = true;
        Some(entity)
    }
    pub fn workspace(
        name: &str,
        mut capture: WorkspaceCapture,
        baseline: DockLayout,
        now: u64,
    ) -> Self {
        for revision in capture
            .history
            .revisions
            .values_mut()
            .filter(|r| r.timestamp_ms == 0)
        {
            revision.timestamp_ms = now;
        }
        Self {
            id: new_id(),
            metadata: Metadata::new(ItemKind::Workspace, name, now),
            content: ItemContent::Workspace {
                history: capture.history,
                baseline: Box::new(baseline),
            },
            working: Some(capture.working),
        }
    }
    pub fn toolbar(definition: ToolbarDefinition, now: u64) -> Self {
        Self {
            id: new_id(),
            metadata: Metadata::new(ItemKind::Toolbar, &definition.name, now),
            working: None,
            content: ItemContent::Toolbar { definition },
        }
    }
    /// Built-in workspaces restore the current shipped preset for this host.
    /// Copies and custom workspaces retain their original saved baseline.
    pub fn starting_layout(&self, platform: layer_ui::Platform) -> Result<DockLayout, StoreError> {
        let ItemContent::Workspace { baseline, .. } = &self.content else {
            return Err(StoreError::invalid("Choose a workspace."));
        };
        if self.metadata.builtin
            && let Some((_, preset)) = DEFAULT_WORKSPACES.iter().find(|(id, _)| *id == self.id)
        {
            return Ok(preset.layout(platform));
        }
        Ok(baseline.as_ref().clone())
    }
    pub fn capture(&self) -> Result<WorkspaceCapture, StoreError> {
        match (&self.content, &self.working) {
            (ItemContent::Workspace { history, .. }, Some(working)) => Ok(WorkspaceCapture {
                history: history.clone(),
                working: working.clone(),
            }),
            _ => Err(StoreError::invalid("Choose a workspace.")),
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        if self.id.is_empty() || self.id.len() > 128 {
            return Err(StoreError::invalid("Invalid item identity."));
        }
        self.metadata.validate()?;
        self.content.validate()?;
        if self.metadata.kind != self.content.kind() {
            return Err(StoreError::invalid("Inconsistent item type."));
        }
        if self.metadata.kind == ItemKind::Workspace {
            self.capture()?.validate().map_err(StoreError::invalid)?;
        } else if self.working.is_some() {
            return Err(StoreError::invalid(
                "Saved toolbars cannot contain workspace tool settings.",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generations {
    #[serde(with = "counter")]
    pub metadata: u64,
    #[serde(with = "counter")]
    pub layout: u64,
    #[serde(with = "counter")]
    pub working: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub id: String,
    pub epoch: String,
}
impl Owner {
    pub fn fresh() -> Self {
        Self {
            id: new_id(),
            epoch: new_id(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub owner: Owner,
    #[serde(with = "counter")]
    pub fence: u64,
    #[serde(with = "counter")]
    pub expires_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredEntity {
    pub entity: Entity,
    pub generations: Generations,
    pub claim: Option<Claim>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemSummary {
    pub id: String,
    pub metadata: Metadata,
    pub generations: Generations,
    pub claim: Option<Claim>,
    pub error: Option<String>,
}
