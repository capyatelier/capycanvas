# Supplemental binary notices

Cargo omits the root license texts from these published crate archives. The
package collector fails for any unreviewed omission. These version-specific
entries supplement the exact locally resolved Windows dependency tree.

- spirv 0.4.0+sdk-1.4.341.0: Apache-2.0, from
  [rspirv at its crate source commit](https://github.com/gfx-rs/rspirv/blob/8afc3d0ac8e158128cd1410bb2e4b4c26ab11bb4/LICENSE).
- gl_generator 0.14.0: Apache-2.0; copyright 2015 Brendan Zabarauskas and
  the gl-rs developers, retained in the published lib.rs header.
  [Upstream license](https://github.com/brendanzab/gl-rs/blob/ea503e8d5fb6d73c6030e6191ce738cd3bf3433e/LICENSE).
- khronos_api 3.1.0: Apache-2.0. Its registry input notices are collected
  separately from the XML headers. The ANGLE BSD license follows its pinned
  [submodule commit](https://github.com/google/angle/blob/7403dd2cd3764fe96660fe09892e764e9ae1dbca/LICENSE).
  SHA-256: bf4da21bd20bcfb5b60b7ecc67fa864a79be049e21d6178076887f178dd6c71a.
  [Upstream license](https://github.com/brendanzab/gl-rs/blob/f150967b1c44ae888e6676f93f639ebc82771bdc/LICENSE).
- profiling and profiling-procmacros 1.0.18: the included MIT option is copied verbatim from
  [its crate source commit](https://github.com/aclysma/profiling/blob/8271551172eb6fa4cba47369aedd93790c623df9/LICENSE-MIT).
  SHA-256: c8167fdeeed46d3f244d3f85c5bf998ce889343691c32be2c61a8bc4b5c08333.

The Apache entries use the repository's unmodified Apache-2.0 license text.
The collector also includes Rust's installed standard-library copyright report,
NuGet license/notice files, the Visual C++ redistribution list, and the project's
source, asset and branding terms. Generated dependency inventories remain local.

The portable photo dependencies also require:

- atomig-macro 0.4.0: the original MIT notice at
  [the recorded source revision](https://github.com/LukasKalbertodt/atomig/blob/abd42e50a6a1b0615505189a780fcb5da19d7385/LICENSE-MIT).
  SHA-256: 6f54826bbf67526f2d41937c1904ae2de63ef5707b80c68deadbf073b5669c40.
- simd_helpers 0.1.0: the original MIT notice at the license-only follow-up
  [revision pinned in the shared build configuration](https://github.com/lu-zero/simd_helpers/blob/82040194cd05affb060bf94d6f19f82a771d07fb/LICENSE).
  SHA-256: d69f24ad84ec2ade64c0b68bdb31b41170e997b158370342056918329cc9af1e.
- zune-core 0.4.12, zune-inflate 0.2.54 and zune-jpeg 0.4.21: the checked-in Zlib notice from
  tools/build/licenses, using the same SHA-256 as tools/build/about.toml.
  Checksums refer to original LF text; Windows checkout CRLF is normalized.

Vendored crates use their actual source notices and manifests; registry copies
are not substituted for patched sources. THIRD_PARTY_NOTICES.md retains the
portable photo dependencies' full notices and AV1 patent statement.
