# Zen mode preferences — GTK, Web and Android

The Zen mode group in Settings → Appearance has two switches with self-contained
titles and no descriptions:

- **Show Capy in Zen mode** defaults to on. A standalone top-left Capy remains
  available while the title bar is hidden, including customized title bars.
- **Reveal panels near screen edges** defaults to off. Enabling it restores
  occupied-edge tap/hover reveal and edge reveal during floating-panel movement.

Rust owns both preferences, saved-setting defaults/reset and visibility policy.
Hosts provide the visible Capy bounds so edge reveal does not replace the button
under a click. With both switches off, the configured Zen keyboard shortcut
still exits the mode. Zen does not move the camera or rearrange the workspace.

## Original verification

Validated as local changes based on `5004f1be` in `capycanvas3` on 2026-09-20.
These changes had not yet been committed to `origin/main`.

- Shared Rust: 499 `layer-ui` tests and 28 `layer-host` tests passed (one existing
  ignored host test). The five settings tests also passed after the title update.
- GTK: release test build and `native_zen_behaviors` passed under the private
  Wayland/GPU runner, including all four options, both themes and settings rows.
- Android: debug app, test APK and `lintDebug` passed for `arm64-v8a`.
  `AndroidTitleBarTest#zenCapyAndEdgeRevealPreferences` passed on the Huion
  (`OK (1 test)`, 67 seconds): all four combinations in both themes, native
  touch/mouse/stylus event dispatch, Capy/keyboard exit, unchanged layout/camera,
  and preferences retained after closing and relaunching the activity.
- Web: the Huion Chrome test passed all four combinations in both themes, with
  injected touch, mouse and pen events, one-tap Capy exit with either edge policy,
  keyboard recovery, a title bar without Capy, and unchanged layout/camera.

Physical device: Huion Kamvas Pad 12 (`KP1202`, serial `G7DL2S300241`). Web ran
in its Chrome 143 at a 1200×680 CSS viewport and DPR 2. These are automated tests
on the tablet, not manual physical-pen accuracy measurements. The separate
Android test application is `art.capycanvas.zentest` (Capy Zen Test).

Local logs and screenshots are in `artifacts/zen-huion/`. Other Chrome tabs and
existing Android app data were preserved. Recovery prompts for previous Web
test drawings were dismissed with **Keep for Later**.

## Integration with optimized main

Revalidated on 2026-09-20 after applying the Zen changes to `efe76d57`
(the Huion UI startup optimizations). The Web startup merge retains both the
standalone Zen button and the startup performance mark.

- Shared Rust: 504 `layer-ui` and 28 `layer-host` tests passed; one existing host
  test remains ignored.
- Web: release Wasm build, pointer and workspace-client Node tests passed.
  The full Zen test passed again in a dedicated Huion Chrome tab: defaults,
  both switches, all four combinations in both themes, touch/mouse/pen,
  one-tap Capy exit, keyboard recovery and customized title bars.
- Android: arm64 debug app/test builds and `lintDebug` passed.
  `AndroidTitleBarTest#zenCapyAndEdgeRevealPreferences` passed on the Huion
  in 78.845 seconds, including activity-relaunch persistence.
- GTK: release test build and `native_zen_behaviors` passed with a clean process
  exit. Initial runs passed their assertions but crashed during process shutdown;
  the test now calls the existing shader-worker shutdown function, matching the
  application and fullscreen test cleanup. Production shutdown is unchanged.

Artifacts are in `artifacts/zen-main-integration-2026-09-20/`. Android used the
isolated `art.capycanvas.zenintegration` package; both temporary APKs were removed
after testing. The temporary Web tab and ADB connections were also removed. This
integration validation did not replace the previously deployed applications.

## Web Capy exit transition

On 2026-09-20, Huion frame sampling reproduced a Capy flash when leaving Zen:
the standalone button disappeared while the title-bar Capy inherited the
header's 180 ms opacity transition, starting with two fully transparent frames.
Web now fades individual header items and switches the Capy immediately.
The full Huion Zen test passes with frame-by-frame assertions that Capy stays
opaque while other header controls still fade, for both themes, both edge-reveal
settings, and touch/mouse/pen exits. Reduced-motion hide/reveal remains immediate.
Logs and captures are in
`artifacts/zen-capy-flash-2026-09-20/`.

## Reproduce

Build Web with `bash apps/layer-web/build.sh`, serve `apps/layer-web` locally,
and use ADB reverse forwarding to open a dedicated test page on the Huion.
Forward Chrome's debugger, then select only that test tab:

```sh
adb -s G7DL2S300241 forward tcp:9240 localabstract:chrome_devtools_remote
node apps/layer-web/zen-tablet.test.mjs TEST_TAB_ID
adb -s G7DL2S300241 shell am instrument -w \
  -e class art.capycanvas.AndroidTitleBarTest#zenCapyAndEdgeRevealPreferences \
  art.capycanvas.zentest.test/androidx.test.runner.AndroidJUnitRunner
```

The native test isolates its workspace/recovery stores and restores preferences.
Build its APKs with `-PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.zentest`.
