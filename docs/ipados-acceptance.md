# iPadOS acceptance tracker

The goal is a complete native iPad app with every feature exposed by the shared
and existing-host UI, exact main-editor design intent, and demonstrated hardware
performance. Initial compilation, a screenshot, or basic drawing is not completion.

## Milestones

- [x] Shared `layer-host` extraction, Android target compile and seven host tests.
- [x] Apple C ABI and native Metal library builds for device and Mac.
- [x] Incorporate the incoming figure, ruler and affine-transform foundation.
- [x] Native Swift editor shell builds for iPadOS simulator/device and AppKit.
- [x] Simulator launches with a live Metal canvas behind the header and panels.
- [x] Simulator launch test passes: Metal readiness, Zen-button dimensions and retained screenshot.
- [x] Personal Team certificate verified; iPad build signed and installed.
- [x] Physical iPad app launches after the developer account is trusted on the device.
- [ ] Full native UI inventory mapped to implementation and test evidence.
- [ ] Every inventory entry implemented and verified; nothing hidden to claim parity.
- [ ] Pencil/touch/keyboard behavior and all supported sensor corrections verified.
- [ ] Document save/reopen/autosave/recovery and settings/workspace persistence verified.
- [ ] Rotation, background/foreground, window/surface replacement and memory pressure verified.
- [ ] Matching Chrome/native captures and full pixel differences accepted.
- [ ] Physical-device performance matrix and ten-minute sustained sessions accepted.

The inventory example (`cargo run -p layer-host --example inventory`) emits the
current catalog, command list, initial workspace and settings views. Extend the
inventory through dynamic states and existing host controls: it is a starting
point, not a completeness proof. Pull changes from other ports at milestones;
new figure/ruler/transform controls and future shared additions are in scope.

Required functional coverage includes menus, actions, commands, every tool and
brush, layers/masks/blending, selections/fills, figures/rulers/transforms, filters,
all dialogs and panels, docking/customization, shortcuts and Zen. Settings may
use native navigation and styling but retain all shared behavior.

Visual fixtures must match logical size, pixel scale, application/document state
and sRGB color handling. Compare complete images and retain overlays/heatmaps,
plus geometry checks within one logical pixel. Only narrowly documented text,
shadow rasterization and unavoidable system-control accommodations are allowed.
Main-page native system menus/sliders still need visual parity work.

Measure the attached 13-inch M4 iPad at sustained 120 Hz with simple and complex
brushes, prediction and pen-up, including 4K multilayer documents. Record CPU/GPU
work against 8.33 ms and separately measure real presentation cadence and
input-to-present latency. Report p50/p95/p99, maxima, missed deadlines, memory
growth, idle behavior and thermal effects. Include ten-minute sessions; simulator
or desktop timings and averages cannot replace hardware acceptance. Never lower
brush fidelity or omit failing workloads to claim success.

Generated captures and test results belong in ignored `artifacts/ui/parity`.
Signing material and device identifiers remain local. Repository commits should
contain reproducible commands and honest results, not private keys or profiles.
