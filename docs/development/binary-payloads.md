# Binary payload boundaries

Bulk pixels and ICC profiles belong in byte buffers. JSON remains useful for
small, inspectable indices, settings, effect definitions and UI commands.

## Audited paths

- Editable projects, recovery files, raster history, original image samples,
  project ICC profiles and packed brush assets already use binary backing.
- Web whole-project worker transfers now detach current, saved and initial
  selection masks through the shared project selection index. Shared masks
  travel once as zero-padded 64 KiB little-endian word blocks. The editor yields
  after 4 MiB of copies; compression and archive integrity hashing stay in the
  file worker. This closes the remaining path that expanded a 61 MP mask into
  JSON during browser save, recovery, opening and document conversion.
- Original-source and proof ICC profiles in those transfers use a shared
  `ProfileReference` and deduplicated binary buffers. The private worker protocol
  changes with the application; the persisted project format stays at version 7.
- Export preset libraries retain each ICC payload as binary, independently of
  profile-library files. All hosts use `ExportPresets::encode/decode`; old JSON
  libraries remain readable and migrate on the next successful atomic write.
  Existing host storage locations (including legacy `.json` filenames) remain
  stable so an upgrade cannot silently miss a user's presets.
- Workspace components contain UTF-8 configuration JSON, not artwork. Browser
  schema 5 snapshots and version 2 portable packages store these as strings instead of
  JSON arrays of byte values. Version 1 packages and old browser byte arrays
  remain readable with identical content hashes. Native SQLite already stores
  components as BLOBs. Commit/retry protocol encoding remains stable, preserving
  existing receipts and interrupted deliveries.

Small scalar arrays, vector selection contours, shader source, settings and
diagnostic JSONL retain their existing schemas. UI command bridges retain their
established JSON formats, including explicitly requested profile details; bulk
storage and whole-project transfer use the binary paths above.

## Preset envelope

`CAPYPRESETS\x01` (12 bytes), little-endian u32 JSON length, little-endian u32
block count, JSON index, then each block's little-endian u64 length and bytes.
A trailing SHA-256 covers the complete header, index and payload. Readers check
the file budget, block count, lengths, integrity, profile references and total
ICC budget before adopting a library. Truncation, extra data and unused profiles
are errors. The shared envelope helper caps the block count at 65,536; preset
validation additionally limits names/profiles and 16 MiB of retained ICC data.

For a generated 2 MiB profile shared by named/remembered presets, storage drops
from 7,488,092 JSON bytes to 2,097,821 binary bytes (72% smaller). A release desktop
run encoded/decoded it in about 1.2 ms each; device and profile validation costs
are separate from this codec measurement.

Huion KP1202 Chrome qualification of the generated 61 MP fixture retained one
mask for three targets, with 6,328 bytes of worker metadata and 62,325,779 binary
payload bytes (including one 2 MiB ICC shared by proof and an original source).
File-worker read/handoff took 1.73 s; the test's main-heap unpack/archive-write
took 685 ms, and file-worker write took 646 ms. Every mask byte and the complete
reopened archive matched exactly. Production archive writes run in the file worker. These are full
transport/archive timings, separate from tonal classification latency. The
fixture's compressibility determines its roughly 4.83 MB archive size.

Native Huion Android qualification also retained the previous 61 MP targets:
warm tonal adjustments 815–890 ms, Quick Mask adjustment 977 ms and recovery
publication 153 ms, with successful recovery reopening. The preset/profile
ownership tests passed on native Android and GTK. Shared core/UI/workspace tests,
50 Huion IndexedDB/SQLite contract cases, Web/Android builds and Android lint pass.
Apple/Windows Rust hosts compile; their native UIs were not exercised here.

## Reproducible checks

```sh
cargo test --locked -p layer-core -p layer-ui -p layer-workspace \
  --features layer-workspace/native --lib
cargo run --locked --release -p layer-core --example binary_transfer -- \
  apps/layer-web/pkg/binary-transfer-fixture.capy
bash apps/layer-web/build.sh
LAYER_DEVICE_CDP=http://127.0.0.1:9239 LAYER_WEB_URL=http://127.0.0.1:4215/ \
  LAYER_BINARY_FIXTURE_URL=/pkg/binary-transfer-fixture.capy \
  node apps/layer-web/device.test.mjs --binary-transfer
```

Serve `apps/layer-web` and forward the test port and Chrome debugging socket to
the device. The fixture contains generated 9504 × 6336 coverage shared by three
selection targets and opaque synthetic ICC bytes; it tests transport, not CMM
acceptance or rendering. The test leaves the editor's open document untouched.
Use the workspace store contract test for IndexedDB parity. Native Android
`profileLibraryKeepsExactCopiesAndPresetOwnership` and
`exportPresetsPersistAndRestoreEveryDeliveryChoice` cover real CMM-validated
profiles and preset persistence. GTK's
`native_export_presets_save_update_remove_reset_and_remember_after_delivery`
checks the same delivery flow through its native controls.
Run that GTK chooser test with `GDK_DEBUG=no-portals` on the isolated Wayland
display so it can drive the in-process chooser.
