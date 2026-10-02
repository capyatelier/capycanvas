use crate::{Localizer, MessageId};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct CommonCopy {
    pub name: Arc<str>,
    pub cancel: Arc<str>,
    pub apply: Arc<str>,
    pub done: Arc<str>,
    pub save: Arc<str>,
    pub ok: Arc<str>,
    pub close: Arc<str>,
    pub remove: Arc<str>,
    pub delete: Arc<str>,
    pub import: Arc<str>,
    pub create: Arc<str>,
    pub reset: Arc<str>,
    pub back: Arc<str>,
    pub more: Arc<str>,
    pub keep_open: Arc<str>,
    pub choose_file: Arc<str>,
    pub update: Arc<str>,
}
impl CommonCopy {
    pub fn new(localizer: &Localizer) -> Self {
        Self {
            name: localizer.text(MessageId::COMMON_NAME),
            cancel: localizer.text(MessageId::COMMON_CANCEL),
            apply: localizer.text(MessageId::COMMON_APPLY),
            done: localizer.text(MessageId::COMMON_DONE),
            save: localizer.text(MessageId::COMMON_SAVE),
            ok: localizer.text(MessageId::COMMON_OK),
            close: localizer.text(MessageId::COMMON_CLOSE),
            remove: localizer.text(MessageId::COMMON_REMOVE),
            delete: localizer.text(MessageId::COMMON_DELETE),
            import: localizer.text(MessageId::COMMON_IMPORT),
            create: localizer.text(MessageId::COMMON_CREATE),
            reset: localizer.text(MessageId::COMMON_RESET),
            back: localizer.text(MessageId::COMMON_BACK),
            more: localizer.text(MessageId::COMMON_MORE),
            keep_open: localizer.text(MessageId::COMMON_KEEP_OPEN),
            choose_file: localizer.text(MessageId::COMMON_CHOOSE_FILE),
            update: localizer.text(MessageId::COMMON_UPDATE),
        }
    }
}
