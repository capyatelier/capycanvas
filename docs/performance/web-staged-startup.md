# Four-stage WebGPU startup — 2026-09-10

The web host now uses the shared dependency scheduler with a browser compiler:

1. Flat paper composition and viewport presentation.
2. Tiled document composition, loaded stroke assets/pipelines, masks, and the document's filter chains.
3. Current brush and physical eraser variants, including their texture assets and prediction requirements.
4. Remaining brush shaders/assets and the filter catalog. Thumbnails and filter previews wait for this work.

Browser jobs own cloned GPU handles and recipes. They never hold the Wasm session borrow across an await. Jobs run one at a time between display opportunities, with required document and brush jobs taking priority over speculative jobs. GPU error scopes gate readiness; a rejected runtime filter preserves the working catalog. Contacts begun before brush readiness remain suppressed through their release.

The first paper submission gets a separate queue-completion wait and display opportunity before document compilation starts. Merely yielding requestAnimationFrame was insufficient on the tablet: later compilation could join Chrome's GPU command processing and delay the first canvas. Expensive procedural brush textures also moved out of first-canvas initialization; their recipes follow the same dependency priorities.

This is cooperative browser scheduling through wgpu's WebGPU backend. It does not move the Wasm renderer into a worker or add an application shader cache. wgpu 30 submits the browser's synchronous pipeline-creation APIs; a single driver compilation cannot be preempted. The browser retains control of its internal GPU caches.

## Tablet result

Wacom MovinkPad14, Android 15, Qualcomm Adreno 7xx, Chrome 152.0.7977.82. Benchmarked in Chrome on the USB tablet, never using desktop GPU timings. Before/after static builds were frozen and served through the same localhost origin and USB reverse tunnel. Five alternating pairs, each preceded by a complete untimed warmup of that build. Timed reloads bypassed HTTP cache; Chrome GPU caches were retained, matching the earlier benchmark conditions. No browser data was deleted.

| Pair | Before first canvas GPU completion | After | Reduction |
| --- | ---: | ---: | ---: |
| 1 | 496.9 ms | 459.6 ms | 37.3 ms |
| 2 | 509.6 ms | 443.1 ms | 66.5 ms |
| 3 | 505.2 ms | 430.7 ms | 74.5 ms |
| 4 | 523.1 ms | 451.1 ms | 72.0 ms |
| 5 | 488.0 ms | 428.5 ms | 59.5 ms |
| Median | **505.2 ms** | **443.1 ms** | **62.1 ms / 12.3%** |

The new median is also approximately **79 ms / 15% faster than the earlier ~522 ms benchmark**. All five paired runs improved. Median first canvas submission fell from 480.3 ms to 419.5 ms. Pipelines submitted before that canvas fell from **27 to 4**; unused brush, watercolor, export, and tiled-document pipelines are excluded from stage 1. Median CPU work inside GPU initialization dropped from 83.9 ms to 1.1 ms, chiefly by deferring procedural brush assets.

The probe follows command buffers using the `viewport presentation` pipeline and records `GPUQueue.onSubmittedWorkDone()` after that submission. This avoids confusing the earlier upload-only submission with a rendered canvas. It measures completed GPU work relative to navigation, not physical panel scanout or input-to-photon latency. This small warmed-cache sample does not establish factory-cold browser timings. Earlier exploratory runs and cache-miss warmups are retained separately and excluded from the paired table.

Raw probe JSON, Chrome traces, script snapshots, exact build hashes, and numerical results are in `artifacts/web-startup/`. Before WASM SHA-256: `1265d809cb3a7c0dd8cb5a24800ef27448d0ac6f2ad0b9e982ab6db8f8417199`. After: `869d9627d9da1b9f9a7f9c540f34924344e3785b8621c3d325661b3e7b9c08fd`.

## Verification

- Local Chrome with real NVIDIA WebGPU: staged startup test holds a required shader's validation, verifies visible paper and usable settings, then holds an optional shader and verifies painting and camera movement. A contact begun before readiness cannot start painting midway through. A separate document initialized with domain warp validates that filter before enabling document/brush readiness.
- Packaged runtime-filter tests: new filter loading, replacement, metadata updates, catalog merging, and invalid WGSL preserve the last working catalog.
- Packaged compatibility tests: Chrome 131 layout constraints, an escaped browser pipeline exception, and an escaped initialization rejection recover correctly.
- All 20 packaged startup/failure cases pass, including recovery and preserving settings while GPU attachment is pending.
- Native scheduler ordering/promotion and real GPU loaded-filter pixel comparison pass. Shared scene pipelines remain reused across recreated scenes. The Android ARM64 host passes cargo check.

The broad editor smoke test fails at `Zen hover at 600,80`. A separate build of unchanged commit `40d9096` reproduces that same failure, so it predates this startup change; the focused startup/input/filter checks above pass.

The prior Android staged-startup and bounded native pipeline-cache work is a dependency of this milestone. Its physical-device results are recorded in `artifacts/android/cached-startup-validation.md`; this web change does not add web disk caching.
