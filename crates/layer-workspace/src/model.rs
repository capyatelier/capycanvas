use layer_ui::{
    DockLayout, LayoutHistory, PanelConfig, TileStyle, ToolbarTile, WorkspaceCapture,
    WorkspaceWorkingState,
};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 4;
pub const HISTORY_BUDGET_BYTES: u64 = 100 * 1024 * 1024;
pub const TRASH_LIFETIME_MS: u64 = 30 * 24 * 60 * 60 * 1000;
pub const OWNER_LEASE_MS: u64 = 30_000;
pub const OWNER_RENEW_MS: u64 = 10_000;
pub const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;
/// Retained for older saved-layout references; new installs only seed workspaces.
pub const DEFAULT_TEMPLATE_ID: &str = "builtin:default";
/// Stable identities: names and contents of the workspaces remain editable.
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
    DEFAULT_WORKSPACES
        .iter()
        .any(|(workspace, _)| *workspace == id)
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
pub(crate) mod counter {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
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
    Template,
    Toolbar,
}
impl ItemKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Template => "template",
            Self::Toolbar => "toolbar",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Template => "Layout",
            Self::Toolbar => "Toolbar",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataVersion {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(with = "counter")]
    pub timestamp_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub kind: ItemKind,
    pub name: String,
    pub description: String,
    /// Included items cannot be deleted. Included layouts/toolbars are also
    /// read-only; included workspaces can be renamed and edited normally.
    pub builtin: bool,
    #[serde(with = "counter")]
    pub created_at_ms: u64,
    #[serde(with = "counter")]
    pub modified_at_ms: u64,
    #[serde(with = "counter")]
    pub last_used_ms: u64,
    pub deleted_at_ms: Option<u64>,
    pub previous: Vec<MetadataVersion>,
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
            deleted_at_ms: None,
            previous: Vec::new(),
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        validate_name(&self.name)?;
        if self.description.len() > 16_384 || self.previous.len() > 100_000 {
            return Err(StoreError::invalid(
                "Item metadata exceeds supported limits.",
            ));
        }
        for v in &self.previous {
            validate_name(&v.name)?;
            if v.description.len() > 16_384 {
                return Err(StoreError::invalid("Description is too long."));
            }
        }
        Ok(())
    }
    pub fn rename(&mut self, name: &str, description: &str, now: u64) -> Result<(), StoreError> {
        validate_name(name.trim())?;
        if description.len() > 16_384 || self.previous.len() >= 100_000 {
            return Err(StoreError::invalid(
                "Item metadata exceeds supported limits.",
            ));
        }
        if self.read_only() {
            return Err(StoreError::invalid(
                "Load this included layout into a workspace to customize it.",
            ));
        }
        if name.trim() == self.name && description == self.description {
            return Ok(());
        }
        self.previous.push(MetadataVersion {
            id: new_id(),
            name: self.name.clone(),
            description: self.description.clone(),
            timestamp_ms: self.modified_at_ms,
        });
        self.name = name.trim().into();
        self.description = description.into();
        self.modified_at_ms = now;
        self.validate()
    }

    pub fn read_only(&self) -> bool {
        self.builtin && self.kind != ItemKind::Workspace
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateOrigin {
    pub id: String,
    pub version: String,
    pub name: String,
    pub timestamp_ms: u64,
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
        let mut layout = DockLayout::default();
        let panel = layout
            .panels
            .iter_mut()
            .find(|p| p.id == layer_ui::Panel::Toolbar)
            .unwrap();
        panel.content = layer_ui::PanelContent::Toolbar {
            name: self.name.clone(),
            tiles: self.tiles.clone(),
        };
        panel.tile_style = self.tile_style;
        panel.hide_tab = self.hide_tab;
        // Validate each configured action without importing foreign local tile IDs.
        let mut json = serde_json::to_value(layout)?;
        let next = self
            .tiles
            .iter()
            .map(|t| t.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| StoreError::invalid("Toolbar identities are exhausted."))?;
        json["next_tile_id"] = serde_json::json!(next);
        serde_json::from_value::<DockLayout>(json)?
            .validate()
            .map_err(StoreError::invalid)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReusableContent {
    Layout { layout: DockLayout },
    Toolbar { definition: ToolbarDefinition },
}
impl ReusableContent {
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Layout { .. } => ItemKind::Template,
            Self::Toolbar { .. } => ItemKind::Toolbar,
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        match self {
            Self::Layout { layout } => validate_stored_layout(layout),
            Self::Toolbar { definition } => definition.validate(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReusableVersion {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: ReusableContent,
    #[serde(with = "counter")]
    pub timestamp_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemContent {
    Workspace {
        history: LayoutHistory,
        baseline: DockLayout,
        origin: Option<TemplateOrigin>,
    },
    Reusable {
        current: ReusableVersion,
        previous: Vec<ReusableVersion>,
    },
}
impl ItemContent {
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Workspace { .. } => ItemKind::Workspace,
            Self::Reusable { current, .. } => current.content.kind(),
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        match self {
            Self::Workspace {
                history, baseline, ..
            } => {
                history.validate().map_err(StoreError::invalid)?;
                validate_stored_layout(baseline)
            }
            Self::Reusable { current, previous } => {
                current.content.validate()?;
                if previous.len() > 100_000 {
                    return Err(StoreError::invalid("Too many previous versions."));
                }
                for v in previous {
                    v.content.validate()?;
                    if v.content.kind() != current.content.kind() {
                        return Err(StoreError::invalid("Inconsistent library version."));
                    }
                }
                Ok(())
            }
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
    pub fn workspace(
        name: &str,
        mut capture: WorkspaceCapture,
        baseline: DockLayout,
        origin: Option<TemplateOrigin>,
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
                baseline,
                origin,
            },
            working: Some(capture.working),
        }
    }
    pub fn reusable(name: &str, description: &str, content: ReusableContent, now: u64) -> Self {
        let mut metadata = Metadata::new(content.kind(), name, now);
        metadata.description = description.into();
        Self {
            id: new_id(),
            metadata,
            working: None,
            content: ItemContent::Reusable {
                current: ReusableVersion {
                    id: new_id(),
                    name: name.trim().into(),
                    description: description.into(),
                    content,
                    timestamp_ms: now,
                },
                previous: Vec::new(),
            },
        }
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
                "Saved layouts and toolbars cannot contain workspace tool settings.",
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
