//! Portable color services. Hosts create transforms on their file/render workers
//! and reuse them for bounded strips. No display transform changes document data.
mod icc;
pub use icc::*;
pub mod photo;
pub mod screen;
mod photo_project;
pub use photo_project::{assume_source_profile, photo_project};
mod resize;
pub use resize::{AreaPreview, RowResampler};
mod output_rows;
pub use output_rows::{build_local_tone_guide, encode_working_rows_with_guide, preview_encoded_rows, WorkingRowsOptions};

mod rasterize;
pub use rasterize::rasterize_source;
mod document;
pub use document::{DocumentColorChange, PreparedDocumentColor, prepare_document_color};

mod document_info;
pub use document_info::{DocumentInfo, InspectedDocumentInfo, InspectedSourceInfo};

mod flatten;
pub use flatten::flattened_document;

mod clip;
pub use clip::{ClipRows, source_png, srgb_png_interpretation, write_clip_rows};

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
