# Apple Diagnostics memory handoff

[Apple development](apple.md) · [Apple porting guide](../APPLE_PORTING_GUIDE.md)

Check Diagnostics on an Apple Silicon Mac and a reserved physical iPad, in light
and dark. Delete this handoff once both hosts pass the checks below.

## What changes

Diagnostics presents the shared timing rows, the existing chart after GPU · ms,
then FPS, a separator, the Memory section, another separator and a filled,
start-aligned recording button at its natural width. Timing and memory share one
row view. Memory uses a SwiftUI Canvas bar and pill badges; tooltips use plain
`.help` text and accessibility hints.

| File | Change |
| --- | --- |
| `apps/layer-apple/Shared/Editor/NavigatorPanel.swift` | Reusable readout rows, bars, badges and section; chart help and ID; recording button styling. |
| `apps/layer-apple/Shared/Tests/NavigatorControlChecks.swift` | Checks a numeric `stats-value-0` and `renderer-memory-row-0` instead of the removed Frames value. |

No project-file changes are needed.

## Check on both hosts

- Open Diagnostics from Navigator, leave and reopen it, resize its panel to about
  242 px and switch between light and dark. Compare with the approved Web panel
  and GTK reference: timing spacing 6 px, memory spacing 8 px, text-to-bar gap
  3 px, bar box 12 px with a centered 6 px rounded track.
- Check that labels and the dim heading note end with an ellipsis when needed;
  values stay on one line, use tabular digits and remain right-aligned. Check
  semibold headings and badges, theme colors, separators and the ordinary filled
  recording button. Start and stop a recording through the existing flow.
- Apple Silicon shows one **Memory** bar whose tooltip names the GPU and has a
  Graphics line; there is no GPU row. Values and warnings come from the model.
- On Mac, hover CPU, GPU, FPS, the Memory bar and the Memory heading.
  Multi-line `.help` shows their plain descriptions. iPad has no hover help;
  check the descriptions with VoiceOver and the normal touch journey.
- Inspect `renderer-memory`, `renderer-memory-heading`,
  `renderer-memory-row-<i>`, `renderer-memory-value-<i>` and, where a bar exists,
  `renderer-memory-bar-<i>`. Keep timing IDs `stats-value-<i>`, chart ID
  `renderer-stats-chart` and button ID `stroke-recording`.
- Capture both themes and Mac help text, then review the complete diff against
  the [commit guide](../COMMIT_GUIDE.md#review-the-diff).

## Commands

Run from the repository root on a Mac with the prerequisites in
[Apple development](apple.md). Set `CAPY_APPLE_TEAM` to its development team.
Keep build output outside the worktree and use the worktree's isolated app ID.

```sh
CAPY_DIAGNOSTICS_APP_ID="$(tools/devices/devices.py appid)"
CARGO_TARGET_DIR="$HOME/.cache/capy/diagnostics-memory-rust-check" cargo test --locked -p layer-apple --lib -- --test-threads=1
CAPY_DERIVED_DATA="$HOME/.cache/capy/diagnostics-memory-mac" bash apps/layer-apple/scripts/build.sh macos \
  CAPY_APPLE_BUNDLE_ID="$CAPY_DIAGNOSTICS_APP_ID" \
  CAPY_RUST_BUILD_ROOT="$HOME/.cache/capy/diagnostics-memory-rust-mac"
CAPY_DERIVED_DATA="$HOME/.cache/capy/diagnostics-memory-simulator" bash apps/layer-apple/scripts/build.sh simulator \
  CAPY_APPLE_BUNDLE_ID="$CAPY_DIAGNOSTICS_APP_ID" \
  CAPY_RUST_BUILD_ROOT="$HOME/.cache/capy/diagnostics-memory-rust-simulator"
xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj -scheme CapyCanvas-Mac \
  -destination 'platform=macOS,arch=arm64' -derivedDataPath "$HOME/.cache/capy/diagnostics-memory-mac" \
  CAPY_APPLE_BUNDLE_ID="$CAPY_DIAGNOSTICS_APP_ID" \
  CAPY_RUST_BUILD_ROOT="$HOME/.cache/capy/diagnostics-memory-rust-mac" \
  DEVELOPMENT_TEAM="$CAPY_APPLE_TEAM" CODE_SIGN_IDENTITY='Apple Development' \
  -only-testing:CapyCanvas-MacTests/EditorLaunchTests/testNavigatorAndDiagnostics test
```

The XCTest journey starts in light; also walk the checklist manually in dark.
For the physical iPad, use the isolated bundle ID exported by the device runner
and supply the development team configured for that Mac:

```sh
tools/devices/devices.py --platform apple reserve "$CAPY_TEST_IPAD"
tools/devices/devices.py --platform apple run "$CAPY_TEST_IPAD" -- sh -c '
  xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj -scheme CapyCanvas-iPad \
    -destination "platform=iOS,id=$CAPY_APPLE_DEVICE_ID" \
    -derivedDataPath "$HOME/.cache/capy/diagnostics-memory-ipad" \
    CAPY_RUST_BUILD_ROOT="$HOME/.cache/capy/diagnostics-memory-rust-ipad" \
    CAPY_APPLE_BUNDLE_ID="$CAPY_APPLE_BUNDLE_ID" DEVELOPMENT_TEAM="$CAPY_APPLE_TEAM" \
    -only-testing:CapyCanvas-iPadTests/EditorLaunchTests/testNavigatorAndDiagnostics test
'
tools/devices/devices.py --platform apple release "$CAPY_TEST_IPAD"
```

Use the existing test application's private storage. Preserve installed artist
apps and documents. Linux Rust results and baseline failure reproductions are
recorded in `artifacts/diagnostics-memory-port.local.md`; they do not validate
Swift compilation, Metal presentation, macOS help or iPad interaction.
