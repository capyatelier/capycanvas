use layer_ui::{
    DockLayout, LayoutHistory, PanelConfig, TileStyle, ToolbarTile, WorkspaceCapture,
    WorkspaceWorkingState,
};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 8;
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
        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::EnterANameOf1100CharactersWithoutLeadingOrTrailingSpaces));
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRefusal {
    ToolbarNameMissing,
    ToolbarNameInvalid,
    ToolbarNameCollision,
    InvalidToolbarIdentity,
    ANewerVersionOfCapyCanvasUpdatedWorkspaceStorage,
    AReferencedWorkspaceResourceIsMissing,
    AllDefaultWorkspacesAreOpenInOtherWindows,
    AnInterruptedResourceIsMissing,
    AnItemMayOnlyBeWrittenOnceInOneOperation,
    AnItemWithThisNameAlreadyExists,
    ChooseADifferentReplacementWorkspace,
    ChooseASavedToolbar,
    ChooseAToolbar,
    ChooseAWorkspace,
    ChooseAnItem,
    ChooseOneOfTheListedOptions,
    CloseTheWindowFirst,
    DuplicateOrInvalidToolbarTileIdentity,
    EnterANameOf1100CharactersWithoutLeadingOrTrailingSpaces,
    IncludedLayoutsAndWorkspacesCannotBeDeleted,
    IncludedWorkspacesCannotBeDeleted,
    IncludedWorkspacesCannotBeRenamed,
    InconsistentItemType,
    InconsistentMetadataPayload,
    InvalidItemIdentity,
    InvalidWorkspace,
    InvalidWorkspaceComponent,
    InvalidWorkspaceGeneration,
    InvalidWorkspaceSwitcherOrder,
    ItemMetadataExceedsSupportedLimits,
    ItemTypeCannotChange,
    NoRecoverableItemsWereFound,
    NoReplacementWorkspaceIsAvailable,
    NoWorkspaceIsActive,
    NoWorkspacesAreAvailable,
    OpenAWorkspace,
    OpenAWorkspaceBeforeEditingItsSwitcher,
    OpenAWorkspaceDialogFirst,
    OpenAWorkspaceFirst,
    OwnershipExpiredWhileASaveWasAwaitingConfirmation,
    ReplacementBindingIsUnavailable,
    ReusableItemsCannotStoreWorkingValues,
    SaveAToolbarToTheLibraryFirst,
    SavedToolbarsCannotContainWorkspaceToolSettings,
    StoredLayoutsCannotIncludeMeasuredWidgetGeometryOrViewportFitting,
    SwitchToThisWorkspaceBeforeRestoringItsLayout,
    TheInterruptedChangesAlreadyFinishedSaving,
    TheInterruptedItemIsMissingItsLayout,
    TheInterruptedItemIsMissingMetadata,
    TheReplacementWorkspaceIsUnavailable,
    ThereAreNoInterruptedChangesToRecover,
    TheseChangesHaveAlreadyBeenRecoveredOrSaved,
    TheseInterruptedChangesWereAlreadyRecoveredIntoAnIndependentItem,
    ThisActionDoesNotUseAWorkspaceForm,
    ThisItemChangedInAnotherWindow,
    ThisLayoutVersionIsNoLongerRetained,
    ThisWorkspaceChangedWhileTheWindowWasSuspended,
    ThisWorkspaceIsNoLongerAvailable,
    ThisWorkspaceIsOpenInAnotherWindow,
    ThisWorkspaceItemIsNoLongerAvailable,
    ToolbarHasTooManyControls,
    ToolbarIdentitiesAreExhausted,
    UnexpectedInterruptedChangeReply,
    UnexpectedOwnershipReply,
    UnexpectedWorkspaceClaimReply,
    UnexpectedWorkspaceCommitReply,
    UnexpectedWorkspaceListReply,
    UnexpectedWorkspaceLoadReply,
    UnexpectedWorkspaceOrderReply,
    UnexpectedWorkspaceSwitcherReply,
    WaitForTheCurrentWorkspaceOperationToFinish,
    WorkspaceGenerationExhausted,
    WorkspaceOwnershipChangedWhileLoadingTheToolbar,
    WorkspacePreferencesChangedInAnotherWindow,
    WorkspaceSaveIsAlreadyInProgress,
    WorkspaceStorageWasWrittenByAnEarlierVersionOfCapyCanvas,
}
impl WorkspaceRefusal {
    fn message_id(self) -> layer_ui::MessageId {
        match self {
            Self::ToolbarNameMissing => layer_ui::MessageId::WORKSPACE_REFUSAL_TOOLBAR_NAME_MISSING,
            Self::ToolbarNameInvalid => layer_ui::MessageId::WORKSPACE_REFUSAL_TOOLBAR_NAME_INVALID,
            Self::ToolbarNameCollision => layer_ui::MessageId::WORKSPACE_REFUSAL_TOOLBAR_NAME_COLLISION,
            Self::InvalidToolbarIdentity => layer_ui::MessageId::WORKSPACE_REFUSAL_INVALID_TOOLBAR_IDENTITY,
            Self::ANewerVersionOfCapyCanvasUpdatedWorkspaceStorage => layer_ui::MessageId::WORKSPACE_REFUSAL_A_NEWER_VERSION_OF_CAPY_CANVAS_UPDATED_WORKSPACE_STORAGE,
            Self::AReferencedWorkspaceResourceIsMissing => layer_ui::MessageId::WORKSPACE_REFUSAL_A_REFERENCED_WORKSPACE_RESOURCE_IS_MISSING,
            Self::AllDefaultWorkspacesAreOpenInOtherWindows => layer_ui::MessageId::WORKSPACE_REFUSAL_ALL_DEFAULT_WORKSPACES_ARE_OPEN_IN_OTHER_WINDOWS,
            Self::AnInterruptedResourceIsMissing => layer_ui::MessageId::WORKSPACE_REFUSAL_AN_INTERRUPTED_RESOURCE_IS_MISSING,
            Self::AnItemMayOnlyBeWrittenOnceInOneOperation => layer_ui::MessageId::WORKSPACE_REFUSAL_AN_ITEM_MAY_ONLY_BE_WRITTEN_ONCE_IN_ONE_OPERATION,
            Self::AnItemWithThisNameAlreadyExists => layer_ui::MessageId::WORKSPACE_REFUSAL_AN_ITEM_WITH_THIS_NAME_ALREADY_EXISTS,
            Self::ChooseADifferentReplacementWorkspace => layer_ui::MessageId::WORKSPACE_REFUSAL_CHOOSE_A_DIFFERENT_REPLACEMENT_WORKSPACE,
            Self::ChooseASavedToolbar => layer_ui::MessageId::WORKSPACE_REFUSAL_CHOOSE_A_SAVED_TOOLBAR,
            Self::ChooseAToolbar => layer_ui::MessageId::WORKSPACE_REFUSAL_CHOOSE_A_TOOLBAR,
            Self::ChooseAWorkspace => layer_ui::MessageId::WORKSPACE_REFUSAL_CHOOSE_A_WORKSPACE,
            Self::ChooseAnItem => layer_ui::MessageId::WORKSPACE_REFUSAL_CHOOSE_AN_ITEM,
            Self::ChooseOneOfTheListedOptions => layer_ui::MessageId::WORKSPACE_REFUSAL_CHOOSE_ONE_OF_THE_LISTED_OPTIONS,
            Self::CloseTheWindowFirst => layer_ui::MessageId::WORKSPACE_REFUSAL_CLOSE_THE_WINDOW_FIRST,
            Self::DuplicateOrInvalidToolbarTileIdentity => layer_ui::MessageId::WORKSPACE_REFUSAL_DUPLICATE_OR_INVALID_TOOLBAR_TILE_IDENTITY,
            Self::EnterANameOf1100CharactersWithoutLeadingOrTrailingSpaces => layer_ui::MessageId::WORKSPACE_REFUSAL_ENTER_A_NAME_OF_1_100_CHARACTERS_WITHOUT_LEADING_OR_TRAILING_SPACES,
            Self::IncludedLayoutsAndWorkspacesCannotBeDeleted => layer_ui::MessageId::WORKSPACE_REFUSAL_INCLUDED_LAYOUTS_AND_WORKSPACES_CANNOT_BE_DELETED,
            Self::IncludedWorkspacesCannotBeDeleted => layer_ui::MessageId::WORKSPACE_REFUSAL_INCLUDED_WORKSPACES_CANNOT_BE_DELETED,
            Self::IncludedWorkspacesCannotBeRenamed => layer_ui::MessageId::WORKSPACE_REFUSAL_INCLUDED_WORKSPACES_CANNOT_BE_RENAMED,
            Self::InconsistentItemType => layer_ui::MessageId::WORKSPACE_REFUSAL_INCONSISTENT_ITEM_TYPE,
            Self::InconsistentMetadataPayload => layer_ui::MessageId::WORKSPACE_REFUSAL_INCONSISTENT_METADATA_PAYLOAD,
            Self::InvalidItemIdentity => layer_ui::MessageId::WORKSPACE_REFUSAL_INVALID_ITEM_IDENTITY,
            Self::InvalidWorkspace => layer_ui::MessageId::WORKSPACE_REFUSAL_INVALID_WORKSPACE,
            Self::InvalidWorkspaceComponent => layer_ui::MessageId::WORKSPACE_REFUSAL_INVALID_WORKSPACE_COMPONENT,
            Self::InvalidWorkspaceGeneration => layer_ui::MessageId::WORKSPACE_REFUSAL_INVALID_WORKSPACE_GENERATION,
            Self::InvalidWorkspaceSwitcherOrder => layer_ui::MessageId::WORKSPACE_REFUSAL_INVALID_WORKSPACE_SWITCHER_ORDER,
            Self::ItemMetadataExceedsSupportedLimits => layer_ui::MessageId::WORKSPACE_REFUSAL_ITEM_METADATA_EXCEEDS_SUPPORTED_LIMITS,
            Self::ItemTypeCannotChange => layer_ui::MessageId::WORKSPACE_REFUSAL_ITEM_TYPE_CANNOT_CHANGE,
            Self::NoRecoverableItemsWereFound => layer_ui::MessageId::WORKSPACE_REFUSAL_NO_RECOVERABLE_ITEMS_WERE_FOUND,
            Self::NoReplacementWorkspaceIsAvailable => layer_ui::MessageId::WORKSPACE_REFUSAL_NO_REPLACEMENT_WORKSPACE_IS_AVAILABLE,
            Self::NoWorkspaceIsActive => layer_ui::MessageId::WORKSPACE_REFUSAL_NO_WORKSPACE_IS_ACTIVE,
            Self::NoWorkspacesAreAvailable => layer_ui::MessageId::WORKSPACE_REFUSAL_NO_WORKSPACES_ARE_AVAILABLE,
            Self::OpenAWorkspace => layer_ui::MessageId::WORKSPACE_REFUSAL_OPEN_A_WORKSPACE,
            Self::OpenAWorkspaceBeforeEditingItsSwitcher => layer_ui::MessageId::WORKSPACE_REFUSAL_OPEN_A_WORKSPACE_BEFORE_EDITING_ITS_SWITCHER,
            Self::OpenAWorkspaceDialogFirst => layer_ui::MessageId::WORKSPACE_REFUSAL_OPEN_A_WORKSPACE_DIALOG_FIRST,
            Self::OpenAWorkspaceFirst => layer_ui::MessageId::WORKSPACE_REFUSAL_OPEN_A_WORKSPACE_FIRST,
            Self::OwnershipExpiredWhileASaveWasAwaitingConfirmation => layer_ui::MessageId::WORKSPACE_REFUSAL_OWNERSHIP_EXPIRED_WHILE_A_SAVE_WAS_AWAITING_CONFIRMATION,
            Self::ReplacementBindingIsUnavailable => layer_ui::MessageId::WORKSPACE_REFUSAL_REPLACEMENT_BINDING_IS_UNAVAILABLE,
            Self::ReusableItemsCannotStoreWorkingValues => layer_ui::MessageId::WORKSPACE_REFUSAL_REUSABLE_ITEMS_CANNOT_STORE_WORKING_VALUES,
            Self::SaveAToolbarToTheLibraryFirst => layer_ui::MessageId::WORKSPACE_REFUSAL_SAVE_A_TOOLBAR_TO_THE_LIBRARY_FIRST,
            Self::SavedToolbarsCannotContainWorkspaceToolSettings => layer_ui::MessageId::WORKSPACE_REFUSAL_SAVED_TOOLBARS_CANNOT_CONTAIN_WORKSPACE_TOOL_SETTINGS,
            Self::StoredLayoutsCannotIncludeMeasuredWidgetGeometryOrViewportFitting => layer_ui::MessageId::WORKSPACE_REFUSAL_STORED_LAYOUTS_CANNOT_INCLUDE_MEASURED_WIDGET_GEOMETRY_OR_VIEWPORT_FITTING,
            Self::SwitchToThisWorkspaceBeforeRestoringItsLayout => layer_ui::MessageId::WORKSPACE_REFUSAL_SWITCH_TO_THIS_WORKSPACE_BEFORE_RESTORING_ITS_LAYOUT,
            Self::TheInterruptedChangesAlreadyFinishedSaving => layer_ui::MessageId::WORKSPACE_REFUSAL_THE_INTERRUPTED_CHANGES_ALREADY_FINISHED_SAVING,
            Self::TheInterruptedItemIsMissingItsLayout => layer_ui::MessageId::WORKSPACE_REFUSAL_THE_INTERRUPTED_ITEM_IS_MISSING_ITS_LAYOUT,
            Self::TheInterruptedItemIsMissingMetadata => layer_ui::MessageId::WORKSPACE_REFUSAL_THE_INTERRUPTED_ITEM_IS_MISSING_METADATA,
            Self::TheReplacementWorkspaceIsUnavailable => layer_ui::MessageId::WORKSPACE_REFUSAL_THE_REPLACEMENT_WORKSPACE_IS_UNAVAILABLE,
            Self::ThereAreNoInterruptedChangesToRecover => layer_ui::MessageId::WORKSPACE_REFUSAL_THERE_ARE_NO_INTERRUPTED_CHANGES_TO_RECOVER,
            Self::TheseChangesHaveAlreadyBeenRecoveredOrSaved => layer_ui::MessageId::WORKSPACE_REFUSAL_THESE_CHANGES_HAVE_ALREADY_BEEN_RECOVERED_OR_SAVED,
            Self::TheseInterruptedChangesWereAlreadyRecoveredIntoAnIndependentItem => layer_ui::MessageId::WORKSPACE_REFUSAL_THESE_INTERRUPTED_CHANGES_WERE_ALREADY_RECOVERED_INTO_AN_INDEPENDENT_ITEM,
            Self::ThisActionDoesNotUseAWorkspaceForm => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_ACTION_DOES_NOT_USE_A_WORKSPACE_FORM,
            Self::ThisItemChangedInAnotherWindow => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_ITEM_CHANGED_IN_ANOTHER_WINDOW,
            Self::ThisLayoutVersionIsNoLongerRetained => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_LAYOUT_VERSION_IS_NO_LONGER_RETAINED,
            Self::ThisWorkspaceChangedWhileTheWindowWasSuspended => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_WORKSPACE_CHANGED_WHILE_THE_WINDOW_WAS_SUSPENDED,
            Self::ThisWorkspaceIsNoLongerAvailable => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_WORKSPACE_IS_NO_LONGER_AVAILABLE,
            Self::ThisWorkspaceIsOpenInAnotherWindow => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_WORKSPACE_IS_OPEN_IN_ANOTHER_WINDOW,
            Self::ThisWorkspaceItemIsNoLongerAvailable => layer_ui::MessageId::WORKSPACE_REFUSAL_THIS_WORKSPACE_ITEM_IS_NO_LONGER_AVAILABLE,
            Self::ToolbarHasTooManyControls => layer_ui::MessageId::WORKSPACE_REFUSAL_TOOLBAR_HAS_TOO_MANY_CONTROLS,
            Self::ToolbarIdentitiesAreExhausted => layer_ui::MessageId::WORKSPACE_REFUSAL_TOOLBAR_IDENTITIES_ARE_EXHAUSTED,
            Self::UnexpectedInterruptedChangeReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_INTERRUPTED_CHANGE_REPLY,
            Self::UnexpectedOwnershipReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_OWNERSHIP_REPLY,
            Self::UnexpectedWorkspaceClaimReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_WORKSPACE_CLAIM_REPLY,
            Self::UnexpectedWorkspaceCommitReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_WORKSPACE_COMMIT_REPLY,
            Self::UnexpectedWorkspaceListReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_WORKSPACE_LIST_REPLY,
            Self::UnexpectedWorkspaceLoadReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_WORKSPACE_LOAD_REPLY,
            Self::UnexpectedWorkspaceOrderReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_WORKSPACE_ORDER_REPLY,
            Self::UnexpectedWorkspaceSwitcherReply => layer_ui::MessageId::WORKSPACE_REFUSAL_UNEXPECTED_WORKSPACE_SWITCHER_REPLY,
            Self::WaitForTheCurrentWorkspaceOperationToFinish => layer_ui::MessageId::WORKSPACE_REFUSAL_WAIT_FOR_THE_CURRENT_WORKSPACE_OPERATION_TO_FINISH,
            Self::WorkspaceGenerationExhausted => layer_ui::MessageId::WORKSPACE_REFUSAL_WORKSPACE_GENERATION_EXHAUSTED,
            Self::WorkspaceOwnershipChangedWhileLoadingTheToolbar => layer_ui::MessageId::WORKSPACE_REFUSAL_WORKSPACE_OWNERSHIP_CHANGED_WHILE_LOADING_THE_TOOLBAR,
            Self::WorkspacePreferencesChangedInAnotherWindow => layer_ui::MessageId::WORKSPACE_REFUSAL_WORKSPACE_PREFERENCES_CHANGED_IN_ANOTHER_WINDOW,
            Self::WorkspaceSaveIsAlreadyInProgress => layer_ui::MessageId::WORKSPACE_REFUSAL_WORKSPACE_SAVE_IS_ALREADY_IN_PROGRESS,
            Self::WorkspaceStorageWasWrittenByAnEarlierVersionOfCapyCanvas => layer_ui::MessageId::WORKSPACE_REFUSAL_WORKSPACE_STORAGE_WAS_WRITTEN_BY_AN_EARLIER_VERSION_OF_CAPY_CANVAS,
        }
    }
    pub fn message(self, localization: &layer_ui::Localizer) -> std::sync::Arc<str> { localization.text(self.message_id()) }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "numeric_detail")]
    pub numeric: Option<layer_ui::NumericError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known: Option<WorkspaceRefusal>,
}
fn numeric_detail<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<layer_ui::NumericError>, D::Error> {
    let numeric = Option::<layer_ui::NumericError>::deserialize(deserializer)?;
    if numeric.as_ref().is_some_and(|reason| !reason.valid()) { return Err(serde::de::Error::custom("invalid_numeric_reason")); }
    Ok(numeric)
}
impl StoreError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            numeric: None, known: None,
        }
    }
    pub fn known(kind: ErrorKind, reason: WorkspaceRefusal) -> Self {
        Self { kind, message: reason.message_id().key().into(), numeric: None, known: Some(reason) }
    }
    pub fn workspace(reason: layer_ui::WorkspaceValidationError) -> Self {
        match reason {
            layer_ui::WorkspaceValidationError::Numeric(reason) => Self { kind: ErrorKind::InvalidData, message: reason.code().into(), numeric: Some(reason), known: None },
            layer_ui::WorkspaceValidationError::ToolbarName(reason) => Self::known(ErrorKind::InvalidData, match reason {
                layer_ui::ToolbarNameRefusal::Missing => WorkspaceRefusal::ToolbarNameMissing,
                layer_ui::ToolbarNameRefusal::Invalid => WorkspaceRefusal::ToolbarNameInvalid,
                layer_ui::ToolbarNameRefusal::Collision => WorkspaceRefusal::ToolbarNameCollision,
            }),
            layer_ui::WorkspaceValidationError::Diagnostic(message) => {
                eprintln!("Workspace admission: {message}");
                Self { kind: ErrorKind::InvalidData, message, numeric: None, known: Some(WorkspaceRefusal::InvalidWorkspace) }
            },
        }
    }
    pub fn localized_message(&self, localization: &layer_ui::Localizer) -> String {
        if let Some(reason) = &self.numeric { return reason.message(localization); }
        if let Some(reason) = self.known { return reason.message(localization).to_string(); }
        eprintln!("Workspace storage {:?}: {}", self.kind, self.message);
        use layer_ui::MessageId as M;
        let id = match self.kind {
            ErrorKind::Conflict => M::WORKSPACE_REFUSAL_THIS_ITEM_CHANGED_IN_ANOTHER_WINDOW,
            ErrorKind::OwnedElsewhere => M::WORKSPACE_REFUSAL_THIS_WORKSPACE_IS_OPEN_IN_ANOTHER_WINDOW,
            ErrorKind::InvalidData => M::WORKSPACE_REFUSAL_INVALID_WORKSPACE,
            ErrorKind::NotFound => M::WORKSPACE_REFUSAL_THIS_WORKSPACE_ITEM_IS_NO_LONGER_AVAILABLE,
            ErrorKind::NameCollision => M::WORKSPACE_REFUSAL_AN_ITEM_WITH_THIS_NAME_ALREADY_EXISTS,
            ErrorKind::FailedWrite => M::WORKSPACE_STORAGE_WRITE_FAILED,
            ErrorKind::StorageFull => M::WORKSPACE_STORAGE_FULL,
            ErrorKind::Unavailable | ErrorKind::UnsupportedSchema => M::WORKSPACE_STORAGE_UNAVAILABLE,
        };
        localization.text(id).to_string()
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidData, message)
    }
    pub fn conflict() -> Self {
        StoreError::known(ErrorKind::Conflict, WorkspaceRefusal::ThisItemChangedInAnotherWindow)
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
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ItemMetadataExceedsSupportedLimits));
        }
        Ok(())
    }
    pub fn rename(&mut self, name: &str, description: &str, now: u64) -> Result<(), StoreError> {
        if self.builtin && self.kind == ItemKind::Workspace {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::IncludedWorkspacesCannotBeRenamed));
        }
        validate_name(name.trim())?;
        if description.len() > 16_384 {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ItemMetadataExceedsSupportedLimits));
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
    pub fn capture(panel: &PanelConfig, localization: &layer_ui::Localizer) -> Result<Self, StoreError> {
        let layer_ui::PanelContent::Toolbar { name, tiles } = &panel.content else {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAToolbar));
        };
        Ok(Self {
            name: name.clone().unwrap_or_else(|| panel.id.localized_label(localization).to_string()),
            tiles: tiles.clone(),
            tile_style: panel.tile_style,
            hide_tab: panel.hide_tab,
        })
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        validate_name(&self.name)?;
        if self.tiles.len() > 4096 {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ToolbarHasTooManyControls));
        }
        if self.tiles.iter().any(|tile| tile.id == u32::MAX) {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ToolbarIdentitiesAreExhausted));
        }
        PanelConfig {
            id: layer_ui::Panel::Toolbar, hide_tab: self.hide_tab, tile_style: self.tile_style,
            content: layer_ui::PanelContent::Toolbar { name: Some(self.name.clone()), tiles: self.tiles.clone() },
        }.validate_admission().map_err(StoreError::workspace)?;
        let mut ids = std::collections::BTreeSet::new();
        if self.tiles.iter().any(|tile| tile.id == 0 || !ids.insert(tile.id)) {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::DuplicateOrInvalidToolbarTileIdentity));
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
        return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::StoredLayoutsCannotIncludeMeasuredWidgetGeometryOrViewportFitting));
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
        localization: &layer_ui::Localizer,
    ) -> Option<Self> {
        let (_, preset) = DEFAULT_WORKSPACES.iter().find(|(key, _)| *key == id)?;
        let layout = preset.layout(platform);
        let mut entity = Self::workspace(
            preset.name(),
            WorkspaceCapture {
                history: LayoutHistory::new(&layout),
                working: preset.working_state_localized(localization),
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
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAWorkspace));
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
            _ => Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAWorkspace)),
        }
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        if self.id.is_empty() || self.id.len() > 128 {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InvalidItemIdentity));
        }
        self.metadata.validate()?;
        self.content.validate()?;
        if self.metadata.kind != self.content.kind() {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::InconsistentItemType));
        }
        if self.metadata.kind == ItemKind::Workspace {
            self.capture()?.validate_structure().map_err(StoreError::workspace)?;
        } else if self.working.is_some() {
            return Err(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::SavedToolbarsCannotContainWorkspaceToolSettings));
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
    pub error: Option<StoreError>,
    pub unavailable_name: bool,
}

impl ItemSummary {
    pub fn display_name(&self, localization: &layer_ui::Localizer) -> String {
        if self.unavailable_name { localization.text(layer_ui::MessageId::WORKSPACE_UNREADABLE_NAME).to_string() }
        else { workspace_display_name(&self.id, &self.metadata, localization) }
    }
}

pub fn workspace_display_name(id: &str, metadata: &Metadata, localization: &layer_ui::Localizer) -> String {
    if metadata.builtin && metadata.kind == ItemKind::Workspace {
        if let Some((_, preset)) = DEFAULT_WORKSPACES.iter().find(|(key, _)| *key == id) {
            use layer_ui::{MessageId as M, WorkspacePreset as P};
            return localization.text(match preset { P::Painter => M::WORKSPACE_BUILTIN_PAINTER, P::Illustrator => M::WORKSPACE_BUILTIN_ILLUSTRATOR, P::Photographer => M::WORKSPACE_BUILTIN_PHOTOGRAPHER }).to_string();
        }
    }
    metadata.name.clone()
}

#[cfg(test)]
mod localization_tests {
    use super::*;

    #[test]
    fn refusal_transport_retains_numeric_payload_and_separates_raw_diagnostics() {
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let numeric = layer_ui::NumericError::Range { label: layer_ui::MessageId::TOOL_SETTING_SIZE.into(), min: 0.5, max: 2048.0 };
        let reason = StoreError::workspace(layer_ui::WorkspaceValidationError::Numeric(numeric.clone()));
        let item = ItemSummary { id: "literal {id} 🖌".into(), metadata: Metadata::new(ItemKind::Toolbar, "literal {name} 🖌", 1), generations: Default::default(), claim: None, error: Some(reason), unavailable_name: false };
        let restored: ItemSummary = serde_json::from_slice(&serde_json::to_vec(&item).unwrap()).unwrap();
        assert_eq!(restored, item);
        assert_eq!(restored.error.unwrap().localized_message(&localization), numeric.message(&localization));
        let raw = StoreError::new(ErrorKind::FailedWrite, "literal driver {detail} 🖌");
        assert_eq!(raw.message, "literal driver {detail} 🖌");
        assert_eq!(raw.localized_message(&localization), localization.text(layer_ui::MessageId::WORKSPACE_STORAGE_WRITE_FAILED).as_ref());
        let known = StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAToolbar);
        assert_eq!(known.localized_message(&localization), "ツールバーを選んでください。");
        assert_ne!(known.localized_message(&localization), known.message);
        let toolbar = ToolbarDefinition { name: "a".repeat(65), tiles: vec![], tile_style: Default::default(), hide_tab: false };
        let reason = toolbar.validate().unwrap_err();
        assert_eq!(reason.known, Some(WorkspaceRefusal::ToolbarNameInvalid));
        assert_eq!(reason.localized_message(&localization), layer_ui::ToolbarNameRefusal::Invalid.message(&localization).as_ref());
    }

    #[test]
    fn builtin_identity_projection_preserves_literal_metadata_and_package_semantics() {
        let mut entity = Entity::included_workspace(DEFAULT_WORKSPACES[0].0, layer_ui::Platform::Gtk, 10, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        entity.metadata.name = "stored suffix 2".into();
        let before = serde_json::to_vec(&entity).unwrap();
        for language in layer_ui::UiLanguage::ALL {
            let localization = layer_ui::Localizer::shared(language);
            assert_eq!(workspace_display_name(&entity.id, &entity.metadata, &localization), localization.text(layer_ui::MessageId::WORKSPACE_BUILTIN_PAINTER).to_string());
            let mut custom = entity.metadata.clone();
            custom.builtin = false;
            custom.name = "Sketch".into();
            assert_eq!(workspace_display_name(&entity.id, &custom, &localization), "Sketch");
            assert_eq!(workspace_display_name("literal-user-workspace", &entity.metadata, &localization), "stored suffix 2");
        }
        assert_eq!(serde_json::to_vec(&entity).unwrap(), before);
        let ItemContent::Workspace { history, .. } = &mut entity.content else { panic!() };
        let mut layout = history.layout().clone();
        let panel = layout.add_toolbar(None, "Sketch 日本語 🎨", &[]).unwrap();
        history.append(&layout, layer_ui::LayoutChange::Automatic);
        layout.rename_toolbar(panel, "Tools").unwrap();
        history.append(&layout, layer_ui::LayoutChange::Automatic);
        let descriptions: Vec<_> = crate::layout_history_versions(history).into_iter().map(|r| r.description).collect();
        let ids: Vec<_> = history.revisions.keys().cloned().collect();
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let bytes = crate::export_package_localized(&entity, &localization).unwrap();
        let imported = crate::import_package(&bytes, crate::PackageKind::WorkspaceBackup, 30).unwrap();
        assert_eq!(imported.metadata.name, localization.text(layer_ui::MessageId::WORKSPACE_BUILTIN_PAINTER).to_string());
        let ItemContent::Workspace { history, .. } = &imported.content else { panic!() };
        assert!(history.revisions.keys().all(|id| !ids.contains(id)));
        let mut expected = descriptions;
        let mut actual: Vec<_> = history.revisions.values().map(|r| r.description.clone()).collect();
        expected.sort_by_key(|d| serde_json::to_string(d).unwrap());
        actual.sort_by_key(|d| serde_json::to_string(d).unwrap());
        assert_eq!(actual, expected);
        history.validate().unwrap();
    }
}

#[cfg(test)]
mod numeric_error_tests {
    use super::*;

    #[test]
    fn stored_numeric_detail_round_trips_and_projects_only_at_the_ui_boundary() {
        let label = "作品 { $literal }\u{202e}１２";
        let numeric = layer_ui::NumericError::Range { label: label.into(), min: 0., max: 1. };
        let error = StoreError::workspace(layer_ui::WorkspaceValidationError::Numeric(numeric.clone()));
        assert_eq!(error.message, "numeric-range");
        let encoded = serde_json::to_string(&error).unwrap();
        let decoded: StoreError = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.numeric, Some(numeric));
        let english = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        assert!(decoded.localized_message(&english).contains(label));
        let reference = layer_ui::NumericError::Range { label: layer_ui::MessageId::COMMON_CANCEL.into(), min: 0., max: 1. };
        let error = StoreError::workspace(reference.into());
        let japanese = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        assert!(error.localized_message(&japanese).contains("キャンセル"));
        assert_ne!(error.localized_message(&english), error.localized_message(&japanese));
        for key in ["not-a-catalog-label", "numeric-range"] {
            let mut invalid = serde_json::to_value(&error).unwrap();
            invalid["numeric"]["label"] = serde_json::json!({"message": key});
            assert!(serde_json::from_value::<StoreError>(invalid).is_err(), "{key}");
        }
    }
}
