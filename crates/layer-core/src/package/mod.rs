mod backing;
mod json;
pub use backing::{ByteRange, ByteSource, ImmutableBacking, RangeState, MAX_RANGE_BYTES};
pub use json::{parse_json, references, remap_references};

#[cfg(test)]
mod tests;
