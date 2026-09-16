//! Portable color services. Hosts create transforms on their file/render workers
//! and reuse them for bounded strips. No display transform changes document data.
mod icc;
pub use icc::*;
pub mod photo;
mod resize;
pub use resize::RowResampler;

mod rasterize;
pub use rasterize::rasterize_source;
mod document;
pub use document::{DocumentColorChange, PreparedDocumentColor, prepare_document_color};
