use crate::{Localizer, MessageId, FluentArgs, DocumentTransportRefusal};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub struct DocumentDeliveryCopy {
    pub clipboard_unavailable: Arc<str>,
    pub clipboard_too_large: Arc<str>,
    pub clipboard_empty: Arc<str>,
    pub images: Arc<str>,
    pub drawing_or_photo: Arc<str>,
    pub drawing_type: Arc<str>,
    pub download_file: Arc<str>,
    pub file_saved: Arc<str>,
    pub download: Arc<str>,
    pub cancelling: Arc<str>,
    pub switching_drawing: Arc<str>,
    pub converted_filename_invalid: Arc<str>,
    pub preparing_converted_copy: Arc<str>,
    pub writing_converted_copy: Arc<str>,
    pub drag_panel: Arc<str>,
    pub move_column: Arc<str>,
    pub move_group: Arc<str>,
    pub panel_tabs: Arc<str>,
    pub resize_dock: Arc<str>,
    pub keymap_too_large: Arc<str>,
    pub switch_operation: Arc<str>,
    pub change_in_progress: Arc<str>,
    pub switch_dialog: Arc<str>,
    pub close_operation: Arc<str>,
    pub selected_changed: Arc<str>,
    pub open_operation: Arc<str>,
    pub batch_opening: Arc<str>,
    pub open_drawings_operation: Arc<str>,
}
impl DocumentDeliveryCopy {
    pub fn new(localization: &Localizer) -> Self {
        Self {
            clipboard_unavailable: localization.text(MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_UNAVAILABLE),
            clipboard_too_large: localization.text(MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_TOO_LARGE),
            clipboard_empty: localization.text(MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_EMPTY),
            images: localization.text(MessageId::DOCUMENTS_DELIVERY_IMAGES),
            drawing_or_photo: localization.text(MessageId::DOCUMENTS_DELIVERY_DRAWING_OR_PHOTO),
            drawing_type: localization.text(MessageId::DOCUMENTS_DELIVERY_DRAWING_TYPE),
            download_file: localization.text(MessageId::DOCUMENTS_DELIVERY_DOWNLOAD_FILE),
            file_saved: localization.text(MessageId::DOCUMENTS_DELIVERY_FILE_SAVED),
            download: localization.text(MessageId::DOCUMENTS_DELIVERY_DOWNLOAD),
            cancelling: localization.text(MessageId::DOCUMENTS_DELIVERY_CANCELLING),
            switching_drawing: localization.text(MessageId::DOCUMENTS_DELIVERY_SWITCHING_DRAWING),
            converted_filename_invalid: localization.text(MessageId::DOCUMENTS_DELIVERY_CONVERTED_FILENAME_INVALID),
            preparing_converted_copy: localization.text(MessageId::DOCUMENTS_DELIVERY_PREPARING_CONVERTED_COPY),
            writing_converted_copy: localization.text(MessageId::DOCUMENTS_DELIVERY_WRITING_CONVERTED_COPY),
            drag_panel: localization.text(MessageId::DOCUMENTS_DELIVERY_DRAG_PANEL),
            move_column: localization.text(MessageId::DOCUMENTS_DELIVERY_MOVE_COLUMN),
            move_group: localization.text(MessageId::DOCUMENTS_DELIVERY_MOVE_GROUP),
            panel_tabs: localization.text(MessageId::DOCUMENTS_DELIVERY_PANEL_TABS),
            resize_dock: localization.text(MessageId::DOCUMENTS_DELIVERY_RESIZE_DOCK),
            keymap_too_large: localization.text(MessageId::DOCUMENTS_DELIVERY_KEYMAP_TOO_LARGE),
            switch_operation: DocumentTransportRefusal::SwitchOperation.message(localization),
            change_in_progress: DocumentTransportRefusal::ChangeInProgress.message(localization),
            switch_dialog: DocumentTransportRefusal::SwitchDialog.message(localization),
            close_operation: DocumentTransportRefusal::CloseOperation.message(localization),
            selected_changed: DocumentTransportRefusal::SelectedChanged.message(localization),
            open_operation: DocumentTransportRefusal::OpenOperation.message(localization),
            batch_opening: DocumentTransportRefusal::BatchOpening.message(localization),
            open_drawings_operation: DocumentTransportRefusal::OpenDrawingsOperation.message(localization),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentDeliveryMessage {
    DownloadConfirm { name: String },
    ClipboardShared { detail: String },
    ClipboardFormats { formats: String },
    PastedImage { extension: String },
    ConvertedName { name: String },
    ExportExtension { extension: String },
    ExportPreferences { detail: String },
    OutputCleanup { detail: String },
    SavePreferences { detail: String },
    RestorePreferences { detail: String },
    MovePanel { name: String },
}
impl DocumentDeliveryMessage {
    pub fn message(&self, localization: &Localizer) -> String {
        let mut args = FluentArgs::new();
        let id = match self {
            Self::DownloadConfirm { name } => { args.set("name", name.as_str()); MessageId::DOCUMENTS_DELIVERY_DOWNLOAD_CONFIRM },
            Self::ClipboardShared { detail } => { args.set("detail", detail.as_str()); MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_SHARED },
            Self::ClipboardFormats { formats } => { args.set("formats", formats.as_str()); MessageId::DOCUMENTS_DELIVERY_CLIPBOARD_FORMATS },
            Self::PastedImage { extension } => { args.set("extension", extension.as_str()); MessageId::DOCUMENTS_DELIVERY_PASTED_IMAGE },
            Self::ConvertedName { name } => { args.set("name", name.as_str()); MessageId::DOCUMENTS_DELIVERY_CONVERTED_NAME },
            Self::ExportExtension { extension } => { args.set("extension", extension.as_str()); MessageId::DOCUMENTS_DELIVERY_EXPORT_EXTENSION },
            Self::ExportPreferences { detail } => { args.set("detail", detail.as_str()); MessageId::DOCUMENTS_DELIVERY_EXPORT_PREFERENCES },
            Self::OutputCleanup { detail } => { args.set("detail", detail.as_str()); MessageId::DOCUMENTS_DELIVERY_OUTPUT_CLEANUP },
            Self::SavePreferences { detail } => { args.set("detail", detail.as_str()); MessageId::DOCUMENTS_DELIVERY_SAVE_PREFERENCES },
            Self::RestorePreferences { detail } => { args.set("detail", detail.as_str()); MessageId::DOCUMENTS_DELIVERY_RESTORE_PREFERENCES },
            Self::MovePanel { name } => { args.set("name", name.as_str()); MessageId::DOCUMENTS_DELIVERY_MOVE_PANEL },
        };
        localization.format(id, &args)
    }
}
