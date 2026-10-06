# Binary payload boundaries

[Editor internals](README.md)

Bulk pixels and ICC profiles belong in byte buffers. JSON remains useful for
small, inspectable indices, settings, effect definitions and UI commands.

## Audited paths

- Editable projects, recovery files, raster history, original image samples,
  project ICC profiles and packed brush assets already use binary backing.
- Web artwork worker transfers use the shared `package::transfer` descriptor,
  final manifest records and bounded transferable buffers. Shared current, saved
  and operation selections travel once as little-endian word payloads. The editor
  yields after 4 MiB of copies; compression and archive integrity hashing stay in
  the file worker. Decoded masks never expand into JSON during save, recovery,
  opening or document conversion.
- Original-source and proof ICC profiles share immutable resource IDs and
  deduplicated binary buffers. The private transfer protocol retains runtime
  identity and verification receipts separately from the final persisted
  [package grammar](../reference/capy-package.md).
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
from 7,488,092 JSON bytes to 2,097,821 binary bytes (72% smaller). 

## Reproducible checks

```sh
cargo test --locked -p layer-core -p layer-ui -p layer-workspace \
  --features layer-workspace/native --lib
cargo run --locked --release -p layer-color --example proof_profiles -- \
  artifacts/binary-transfer/profiles
cargo run --locked --release -p layer-core --example binary_transfer -- \
  apps/layer-web/pkg/binary-transfer-fixture.capy artifacts/binary-transfer/profiles/srgb.icc
bash apps/layer-web/build.sh
LAYER_DEVICE_CDP=http://127.0.0.1:9239 LAYER_WEB_URL=http://127.0.0.1:4215/ \
  LAYER_BINARY_FIXTURE_URL=/pkg/binary-transfer-fixture.capy \
  node apps/layer-web/device.test.mjs --binary-transfer
```

Serve `apps/layer-web` and forward the test port and Chrome debugging socket to
the device. The fixture contains a generated 9504 × 6336 saved selection and a
full-size layer mask, plus a valid 2 MiB RGB ICC profile shared by
proof and retained original samples. Its private data tag keeps the payload large
while shared color validation still accepts the profile. The generator writes
`binary-transfer-fixture.capy.icc` beside the package; serve both files. The
journey compares every ICC byte with that companion and checks exact archive
bytes and shared resource identities. It leaves the editor's open drawing
untouched.
Use the workspace store contract test for IndexedDB parity. Native Android
`profileLibraryKeepsExactCopiesAndPresetOwnership` and
`exportPresetsPersistAndRestoreEveryDeliveryChoice` cover real CMM-validated
profiles and preset persistence. GTK's
`native_export_presets_save_update_remove_reset_and_remember_after_delivery`
checks the same delivery flow through its native controls; run it through
`tools/performance/workspace-motion.sh gtk --native-test=<name>`.
