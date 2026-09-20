// This snapshot retains only the portable Rust decoder. Keep the feature name
// so accidental dependency-feature unification fails explicitly, before linking.
#[cfg(feature = "asm")]
compile_error!("Capy Canvas omits rav1d assembly sources; disable default features and use only bitdepth_8/bitdepth_16");

fn main() {}
