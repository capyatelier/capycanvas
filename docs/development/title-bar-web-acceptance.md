# Web title-bar port acceptance

Reference: [GTK handoff](title-bar-web-handoff.md),
[drag convention](../ui/drag-and-reorder.md).

## Milestone 1: shared projection and input

Web now projects the shared header model and platform defaults, with retained
workspace/status controls, plain tool icons, overflow and footer measurements.
The compact component bank uses whole-item immediate pickup after browser slop.
Rust owns geometry, drag previews, validation, keyboard moves and final edits.
The existing tool picker and drawer renderer are reused. Browser IndexedDB and
lease ownership remain separate from native file locking.

Validation on 2026-09-13:

- Web release Wasm build passed.
- 353 shared UI tests and 86 workspace tests with `native` enabled passed,
  including `browser_leases_still_expire_and_fence_stale_writers`.
- 19 Web launcher/packaging tests passed; the header module is fingerprinted.
- `tools/performance/workspace-motion.sh web --title-bar` passed in real headed
  Chrome under isolated Mutter, using CDP mouse, touch and pen streams. It checks
  inert bank clicks, immediate bank/body pickup, outside bank cancellation,
  adding a component, original grab offset through detachment, re-entry,
  singleton removal/return and editor Cancel.
- Captures and machine-readable results: `artifacts/title-bar/web-headed/`.
  Generated artifacts are intentionally ignored by Git.

Headless Chrome completed those pointer assertions but reported a WebGPU
external-instance error and displayed a black canvas. Headed Chrome rendered
the canvas and completed without browser errors. Use the isolated compositor
for the acceptance run on this machine. CDP streams establish browser pointer
arbitration; physical stylus/tablet hardware and mobile browsers remain untested.

Further acceptance journeys are being recorded in the next milestone.
