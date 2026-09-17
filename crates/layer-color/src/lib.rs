//! Portable color services. Hosts create transforms on their file/render workers
//! and reuse them for bounded strips. No display transform changes document data.
mod icc;
pub use icc::*;
pub mod photo;
mod photo_project;
pub use photo_project::{photo_project, assume_source_profile};
mod resize;
pub use resize::{AreaPreview, RowResampler};
mod output_rows;
pub use output_rows::{encode_working_rows, preview_encoded_rows};

mod rasterize;
pub use rasterize::rasterize_source;
mod document;
pub use document::{DocumentColorChange, PreparedDocumentColor, prepare_document_color};

mod document_info;
pub use document_info::DocumentInfo;

mod flatten;
pub use flatten::flattened_document;

/// Validate a new interpretation without changing retained sample ownership.
pub fn repair_source_interpretation(
    mut interpretation: layer_core::color::source::SourceInterpretation,
    working: layer_core::color::RgbSpace,
    profile: layer_core::color::ColorProfile,
) -> Result<layer_core::color::source::SourceInterpretation, String> {
    interpretation.profile = profile;
    interpretation.profile_assumed = false;
    WorkingDecoder::new(&interpretation, working, Default::default())?;
    Ok(interpretation)
}
