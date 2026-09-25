//! Keep the changing cache generation out of the renderer's LLVM code units.
//! The build script retains source-based invalidation; this call runs at startup.

#[inline(never)]
pub fn generation() -> &'static str {
    env!("CAPY_SHADER_GENERATION")
}
