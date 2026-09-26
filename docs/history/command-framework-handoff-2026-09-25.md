# Command framework and command bar handoff

Date: 2026-09-25. Implementation checkpoint: `a7d6f28a`, pushed to `main`.

**Status update, later on 2026-09-25:** the command bar and catalog follow-ups
below are complete. The missing "panel transparency" was the bar using the
opaque popup color with no blur. The bar is now panel glass on GTK, Web and
Android. Menus stay opaque, at the user's direction. Web and Android show the
shared footer and placement. Stage A's coverage ledger, identities and
unavailable reasons are closed; see [command search](../ui/command-search.md)
and audit §7.5. What remains is the keybinding project (stages C–F) and the
Apple and Windows bars. The rest of this document is the original handoff.

The shared catalog and command bars are implemented on GTK, Web and Android.
The broader contextual shortcut, held-action, gesture, device and compatibility
preset project is unfinished. The latest user report is that panel transparency
seems missing. **The user explicitly requested a handoff instead of fixing it
now. No transparency fix or new runtime investigation was performed for this
handoff.** Resume that investigation when implementation resumes.

## Read first

- [Current command-search contract](../ui/command-search.md): implemented
  behavior, boundaries and reproducible host checks.
- [Command/input audit and research](command-input-shortcut-audit-2026-09-25.md):
  original architecture investigation, action inventory, sourced cross-editor
  shortcut tables, context-specific and held-modifier tables, tool classification,
  device feasibility, stages A–F and acceptance criteria.
- [Commit guide](../COMMIT_GUIDE.md) and repository [AGENTS.md](../../AGENTS.md).
- [Drag/reorder convention](../ui/drag-and-reorder.md) if new binding or device
  UI introduces draggable controls. Presets must preserve those rules.

The research file deliberately retains its original snapshot, including the
125-command count and proposed API names. Later implementation checkpoints and
current code supersede those historical statements; `SearchCommands` brought
the enum to 126 entries. Do not mistake proposed behavior for shipped behavior.

## User intent and decisions to preserve

The eventual system should let users discover actions in command search and
connect semantic commands to keyboard shortcuts, held modifiers, gestures,
buttons, pen controls and handheld/Bluetooth devices. Editor-inspired presets
should cover familiar muscle memory, with explicit context and unsupported
actions. A command bar alone does not complete this project.

The user requested implementation/testing on GTK first, followed by Web and
Android, including the attached Huion when useful. They authorized committing
and pushing to `main` after major milestones. Keep the framework and data models
simple and extensible; reuse the current dispatcher, validation, history and
native host capabilities.

For presentation, the user wants a fast, minimal, polished bar with balanced
spacing, existing application styling and subtle feedback. Blender is a useful
comparison; Krita is a secondary reference, not a design to copy mechanically.
Use shared sizes/constants where possible. Avoid result-list metadata overload.

Latest explicit feedback:

1. The GTK bar was too high. It now opens one-fifth down the workspace, clamped
   to 48–192 logical pixels, with a stable top anchor as result counts change.
2. The footer should be useful. **Do not add obvious arrow/Enter/Escape hints.**
   GTK now shows concise command behavior/scope, current setting values and
   ranges, or a menu location. Errors/unavailability take precedence.
3. Panel transparency seems missing. This is unresolved; see the next section.

All user-facing semantic operations belong in the eventual framework, including
held and continuous actions with proper lifecycles. That does not mean every
operation is an executable search result. Raw pen samples, measurement messages,
restoration bookkeeping and host request completions remain internal. Native
text/IME, focus and accessibility ownership must remain intact.

## Immediate follow-up: transparency and host polish

The user's exact report was: “the panel transparency seems missing.” In context
this follows the GTK command-bar refinement, but the precise affected surface,
setting and cause have not been established. Do not assume all workspace panels
regressed or claim this is a confirmed CSS-only defect.

Starting points for the next investigation:

- [GTK command bar](../../apps/layer-linux/src/command_bar.rs): a native
  `gtk::Popover`, parented to the workspace surface, with `command-bar` styling.
- [GTK CSS](../../apps/layer-linux/src/style.css): the workspace sets
  `--popover-bg-color` from `--capy-panel`; existing `.glass` selectors specialize
  dock panels and related surfaces. The command-bar rules currently add spacing
  and row styling but no explicit glass background override. This is a lead,
  not a verified root cause.
- [GTK workspace](../../apps/layer-linux/src/workspace.rs): palette CSS,
  the `glass` class, native scene collection and renderer backdrop publication.
- [GTK glass collection](../../apps/layer-linux/src/glass.rs), shared palette
  and transparency settings, and existing `use_transparency` / `set_transparency`
  helpers in [GTK tests](../../apps/layer-linux/src/tests.rs).

Reproduce over visible artwork with each supported transparency setting and
light/dark themes. Compare against an existing panel/drawer. Trace both color
alpha and backdrop composition: a native popover's separate surface may matter;
changing an RGBA value alone is not proof of correct integration. Prefer the
existing application policy over a command-specific opacity constant.

Acceptance for a future fix: transparency follows the preference and theme,
text/selection remain legible, the opaque setting remains opaque, and opening,
querying, moving/resizing the window and dismissing leave no stale backdrop.
Preserve native input capture, focus, reduced-motion behavior and fast typing.
Use an actual compositor capture; widget-only snapshots may omit the backdrop.

The most recent lower-position/contextual-footer change is **GTK-only** in
presentation. The shared `description` field exists for all hosts, but Web and
Android still render their earlier category/parameter-label footer. Their
positions also retain the earlier host-specific defaults. After settling GTK,
carry the useful footer to those hosts and assess placement against narrow
screens and IME insets. Do not copy a desktop offset blindly onto a tablet.

## Delivered milestones

| Commit | Delivered work |
| --- | --- |
| `35518698` | Shared catalog/search, stable command identities and compatibility mapping, native GTK bar and validation |
| `a0ec03db` | Web native dialog, keyboard/touch/IME/focus behavior, lightweight search publication and native browser tests |
| `9b830b42` | Android Compose bar, lightweight native-host packets, Huion tests and performance correction |
| `a7d6f28a` | Lower GTK placement; shared contextual descriptions and numeric summaries; GTK footer refinement |

Other developers' Windows and Apple commits were integrated before the last
push. Work was done on a detached HEAD; do not assume a local `main` branch is
checked out. Fetch and inspect current status before continuing, preserve
unrelated work and integrate concurrent changes without force-pushing.

## Implementation map

| Area | Main files and responsibilities |
| --- | --- |
| Catalog and search | [command_catalog.rs](../../crates/layer-ui/src/command_catalog.rs): descriptor adapters, stable identities, search/ranking, tool context, availability, numeric entry, invocation and descriptions |
| Shared dispatch | [session.rs](../../crates/layer-ui/src/session.rs): command/input dispatch, focus handling, history and region invalidation; [lib.rs](../../crates/layer-ui/src/lib.rs): public models, command availability and opener |
| Existing bindings | [shortcuts.rs](../../crates/layer-ui/src/shortcuts.rs): keymaps, legacy ID mapping, platform reservations and action equivalence; [tool_settings.rs](../../crates/layer-ui/src/tool_settings.rs) and [numeric.rs](../../crates/layer-ui/src/numeric.rs): applicability, ranges, units and expressions |
| Native publication | [snapshot.rs](../../crates/layer-host/src/snapshot.rs): independent search revision and small open/query/close packets |
| GTK | [command_bar.rs](../../apps/layer-linux/src/command_bar.rs), [workspace.rs](../../apps/layer-linux/src/workspace.rs), [style.css](../../apps/layer-linux/src/style.css) |
| Web | [command-bar.js](../../apps/layer-web/command-bar.js), [app.js](../../apps/layer-web/app.js), [style.css](../../apps/layer-web/style.css), [Wasm interface](../../apps/layer-web/src/lib.rs) |
| Android | [CommandSearch.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/CommandSearch.kt), [CanvasHost.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt), [MainActivity.kt](../../apps/layer-android/app/src/main/java/art/capycanvas/MainActivity.kt) |
| Tests | [Shared catalog tests](../../crates/layer-ui/src/command_catalog_tests.rs), [GTK journey](../../apps/layer-linux/src/command_bar_tests.rs), [Web journey](../../apps/layer-web/command-bar.test.mjs), [Android journey](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidCommandSearchTest.kt) |

### Preserve these implementation contracts

- Catalog providers adapt eight application menus, supported command IDs, tool
  families/choices, brush presets, current tool numeric settings and basic color
  operations. Complex dialogs still use existing host requests and dispatch.
  This is a working catalog foundation, not proof that every host-local action
  or projection has already migrated to a final universal registry.
- Built-in IDs are explicit snake-case wire identities. Nested actions currently
  use canonical serialized action identities, with active-layer IDs and next
  toggle values removed where appropriate. Resources retain explicit IDs.
  Existing PascalCase v1 shortcut IDs have an explicit compatibility mapping.
  Do not change persisted IDs or silently discard old custom actions.
- Invocation accepts known catalog entries, rechecks live availability/targets
  and originating document epoch, and returns to existing `UiAction` dispatch.
  It is not arbitrary JSON execution or a second history stack.
- `CommandKind` currently has Instant, Toggle, Parameter and Held. Held pan
  (`canvas.pan`) is described, excluded from executable search, and still uses
  the old input lifecycle. A generic held/continuous resolver is not implemented.
- `CommandToolContext` reports behavior category, current parameter IDs and
  mask editing. This is not yet the full capability/phase inheritance resolver.
- Search indexes on open, caps queries at 256 characters and results at eight,
  and shows up to five recents/fallbacks on an empty query. Matching is local;
  no filesystem/device/rendering work belongs on the query path.
- `Commit { text }` carries the actual native entry text to avoid executing a
  stale result. Delayed query events cannot exit parameter entry. Rust owns
  `Back`: leave parameter mode first, otherwise close.
- Preserve originating Canvas/Palette/Text focus. Palette Undo targets color
  reorder history; native text Undo is not silently converted to artwork Undo.
  Opening/closing must not leave opener keys or temporary input pressed.
- `COMMAND_SEARCH` invalidation leaves workspace model/content revisions alone.
  GTK updates the popup; Web retains editor DOM; Android observes search state
  separately. Native packets explicitly publish null on close and recover the
  full-model baseline after execution.
- Shared style constants: width 560, inset 12, gap 8, row height 44. Touch hosts
  use 48px/dp targets. Keep names, at most one shortcut and optional checkmarks
  in rows; selected-only detail belongs in the footer.
- GTK uses native popover animation/capture. Web uses a native modal dialog with
  visual-viewport sizing and a 120ms reduced-motion-aware entrance. Android uses
  a stable fullscreen transparent dialog window containing a sized card; IME
  insets constrain it. Resizing the Android native window per query previously
  caused a large latency regression—retain the fixed-window design.
- `description` is generated in the shared catalog, with useful behavior/scope
  text where supplied and a real menu path as fallback. It is intentionally not
  an exhaustive help manual. Numeric summaries retain schema precision and
  display units; rounded toolbar readouts would misstate some allowed bounds.

`SearchCommands` is enabled only on GTK, Web and Android. The original audit
considered six hosts, but Apple/Windows command-bar presentation was not built
in these milestones. Treat those ports as separate follow-up work if requested.

## Tool grouping and remaining implementation sequence

Use behavior categories rather than a shortcut table per brush asset:
Drawing, Erasing, Blending, Warping, Selection, FillGradient, ShapesRulers,
MoveTransform, ColorSampling and Navigation. Existing Ink/Paint/Blend families
remain tool-cycling groups; medium groups such as Pencil/Watercolor/Oil remain
catalog organization. Neither substitutes for semantic shortcut scope.

Within eligible canvas focus, inheritance is intended to run
**canvas → behavior category → tool/submode → active operation**. Use exceptions
for real differences, such as polygon vertex deletion versus marquee geometry.
A selection brush can support size while remaining a Selection tool. Capabilities
come from shared tool schemas, not visible widgets or brush names. Quick Mask
is an editing target, not a separate category for every brush.

| Next work | Gap and success criteria |
| --- | --- |
| Finish bar polish | Investigate the reported transparency issue; retain the accepted lower GTK placement and useful footer; align Web/Android with those decisions while preserving mobile constraints. Verify real composition and input, not only screenshots of individual widgets. |
| Continue catalog convergence | Reconcile each public action source and native-ownership exception with audit §7.1. Preserve IDs, resource targets and old settings; demonstrate equivalent menu/key/button/search behavior. Do not claim full migration merely because search can enumerate menus. |
| C: contextual resolver and held actions | Add explicit logical/physical input, context/capability/phase matching, deterministic precedence, overlapping-context conflict detection and token lifecycles. Preserve current bindings/family cycling. Prove modifier combinations, release/cancel ownership and one-step history for continuous edits. |
| D: gesture and pen adapters | Add touch recognition/arbitration and explicit opt-in pen-button handling, preserving native timing/slop/capture and raw sample identity. Verify hover/contact, cancellation, scrolling and uninterrupted strokes on devices. Native Pencil interactions are a separate host capability. |
| E: device adapters | Start with ordinary keyboard-emulating remotes. Add HID/gamepad/buttons/dials/axes only for verified protocols and platforms. Reconnect, dead zones, repeat and transaction boundaries must work; Bluetooth transport alone does not imply compatibility. |
| F: presets and binding editor | Version source app/OS/layout defaults, expose inheritance/overrides/conflicts and unsupported actions, support deterministic import/export/reset/migration, and preserve user changes. Validate supported muscle-memory rows against source versions and actual runtime behavior. |

For C, retain the precedence described in audit §2.3: native text/IME and
accessibility, existing gesture owner, modal capture, focused control, active
tool/operation, then canvas/application defaults. An equal string is not enough
to identify a binding conflict; contexts and trigger lifecycles must overlap.

Held/continuous actions need `Begin(token) → Update → End` or `Cancel`.
Release routes to the originating owner even after focus/tool/modifiers change.
Cancel on blur, pointer cancellation, disconnect, suspend or document retirement;
make cancellation idempotent. Compose temporary overrides instead of stacking
naive select/restore operations. Specifically test Space→Alt and Alt→Space with
both release orders, Shift/Ctrl combinations, before-contact versus during-drag
behavior, and explicit tool changes while a hold is active. Never synthesize pen
tip-up/down solely because a barrel button changed.

## Research continuity

Keep extending the existing audit instead of starting a second shortcut census.
It already organizes action rows against editor columns, separates tool-specific
contexts and held-modifier combinations, and links official documentation and
tutorials in its source register. It covers desktop, tablet-first, vector and
pixel-art conventions and identifies additional preset candidates.

The inventory is a substantial researched seed, not a certified preset. Retain
the distinction between unimplemented, unavailable, configurable and **VERIFY**.
Unknown is not unsupported. Before shipping each preset, record application
version, OS, keyboard layout, factory/default preferences and exported keymap
where available. Recheck online sources and test the supported behaviors. Do
not promise compatibility with every editor, brush or proprietary BLE device.

## Validation and reproduction

At the earlier full checkpoint, 663 `layer-ui` tests and 32 `layer-host` tests
passed; one pre-existing hardware-specific host test remained ignored. GTK and
Web native command-bar journeys and Android build/lint/instrumentation passed.
For `a7d6f28a`, the eight tests matched by `cargo test --locked -p layer-ui command_`
and the GTK native journey passed. Light/dark, numeric footer and compositor
placement captures were reviewed. **Transparency-setting acceptance was not
established by those captures.** The handoff itself changes documentation only.

Relevant commands, from the repository root:

```sh
cargo test --locked -p layer-ui -p layer-host
bash tools/performance/workspace-motion.sh gtk --native-test=native_command_bar_input --native-storage
LAYER_TEST_ARTIFACTS=/tmp/command-search-web bash tools/performance/workspace-motion.sh web --command-bar
```

For full GTK compositor captures, create an output directory and set
`LAYER_NATIVE_CAPTURE_DIR` when running the GTK command above. The native test
emits `command-bar-placement.png`; widget captures include light, dark and value
entry. Use the isolated compositor harness rather than injecting input into the
user's working desktop. Future transparency coverage should explicitly set the
preference using existing test helpers.

See [Android development](../development/android.md) for prerequisites. In this
session the SDK was `/home/babymastodon/Android/Sdk`; the tested device was a
Huion Kamvas Pad 12, serial `G7DL2S300241`. Verify attachment before use and select
the serial explicitly; another tablet was also attached and was not tested.

```sh
ANDROID_HOME=/home/babymastodon/Android/Sdk apps/layer-android/gradlew \
  -p apps/layer-android -PcapyAbi=arm64-v8a \
  :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug
```

Install both built APKs on the selected device, then run:

```sh
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w \
  -e class art.capycanvas.AndroidCommandSearchTest \
  art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

The Android test injects keyboard, finger and stylus events through native input
dispatch on the physical Huion. It is not proof of manual pen-tip or Bluetooth
device testing. Coverage includes soft-keyboard landscape, portrait layout,
parameter validation/back, repeat opening, menu invocation, retained panels and
outside dismissal. Do not describe the portrait capture as portrait-IME testing.
The installed Android APK predates the final GTK-only presentation refinement.

Warm timing checkpoints were approximately 20ms query-to-frame/draw on Web and
Huion. The latest GTK capture-enabled run measured 10.8ms open-to-paint and
25.3ms query-to-paint p95; synchronous query/model/widget updates were about
0.6ms. These endpoints differ and do not measure physical scanout. Compare
like-for-like runs before diagnosing a regression; preserve bounded search and
avoid workspace rebuilding or per-query native-window resizing.

Before each milestone push, follow the commit guide: inspect the complete diff,
run relevant checks, review affected UI/input, run `git diff --check` and
`sh .githooks/check-commit-messages.sh history HEAD`, and integrate current
`main`. Do not add agent coauthor trailers, bypass hooks, or force-push over
concurrent work. Keep future durable documentation focused on decisions,
coverage and reproduction rather than transient logs.
