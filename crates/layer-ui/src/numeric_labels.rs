use crate::{FluentArgs, Localizer, MessageId};
use serde::Serialize;

#[derive(Serialize)]
pub struct NumericLabels {
    pub edit: String,
    pub decrease: String,
    pub increase: String,
}
impl NumericLabels {
    pub fn new(label: &str, localization: &Localizer) -> Self {
        let mut args = FluentArgs::new(); args.set("label", label);
        Self {
            edit: localization.format(MessageId::NUMERIC_EDIT_LABEL, &args),
            decrease: localization.format(MessageId::NUMERIC_DECREASE_LABEL, &args),
            increase: localization.format(MessageId::NUMERIC_INCREASE_LABEL, &args),
        }
    }
}
