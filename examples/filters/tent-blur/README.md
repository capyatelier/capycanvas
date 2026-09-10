# Runtime filter example

`manifest.json` and the two WGSL files define an original, separable triangular
blur. No Rust kernel, coefficient generator or filter ID registration is needed.
The same metadata declares the Radius control, sampling halo and preview preset.

`prepare.wgsl` produces 17 normalized weight records only when Radius changes.
Both image passes in `tent.wgsl` share that persistent table. This deliberately
uses a different kernel from the application's Gaussian family.

The GPU test `runtime_manifest_loads_a_new_filter_and_its_preparation` reads these
files at runtime, renders the new program and checks that radius zero is identity.
Core loading uses `EffectPackage::parse(manifest)?.resolve(read_module)`; hosts
provide `read_module`, and catalog add/replace is staged without mutating the
published catalog. Renderer validation must precede publication.

These files are covered by the repository's MIT OR Apache-2.0 source license.
