pub const RASTER_TILE_SIZE: u32 = 256;
pub(crate) const SELECTION_CHUNK_BYTES: usize = 65536;

mod backing;
mod json;
pub use backing::{ByteRange, ByteSource, ImmutableBacking, RangeState, MAX_RANGE_BYTES};
pub use json::{parse_json, references, remap_references};

#[cfg(test)]
mod tests;

pub mod archive;

pub mod transport;

pub mod manifest;
pub mod values;
pub mod effect_records;
pub mod selection_records;
pub mod resources;
pub mod preview;
pub mod artwork_records;
pub mod codec;

pub mod transfer;

pub mod session;
pub mod session_transfer;

#[cfg(not(target_arch = "wasm32"))]
pub mod session_store;
