# Windows workspace title bar

Windows now projects the shared workspace title bar with retained WinUI controls.
Window → Customize Title Bar opens the inline component bank. Sketch uses the
shared medium-size header and hides its docked panels and footer. Paint and Photo
retain their own arrangements.

Header items, component availability, overflow, tool activation, drag geometry,
validation and workspace history remain in Rust. WinUI owns native measurements,
pointer capture, hold recognition, focus and window caption regions. The native
color wheel is unchanged; visually imperceptible differences do not justify a
more complicated renderer.

## Accepted behavior

The isolated native fixture covers:

- Whole-item/component placement with native movement slop; inert bank taps and
  holds; pen/touch item menus and same-contact dragging; mouse holds without menus.
- Detached previews, original grab offsets, reattachment, Escape and resize
  cancellation. Drag motion leaves the durable layout unchanged.
- Native Tab/Space, keyboard movement across regions, Delete, all three sizes,
  footer rollback and parent Cancel.
- Searchable multi-selection through the existing Add Tools picker, cancellation,
  insertion order, full tool hit areas, and drawer switching/toggling.
- The complete component bank, singleton omission/removal/re-add, and recovery
  through the menu after removing every header item.
- Whole-item overflow and hidden-item selection at the minimum window size,
  reachable confirmation controls, and preservation of hidden neighbors.
- F11 with native control focus, actual fullscreen state, fullscreen-only Clock,
  one workspace Undo/Redo for Done, restart persistence, and unsaved-edit rollback.
- Native Preferences theme/color validation, retained fields, icon selection,
  search, dependent controls, nested cancellation and immediate reopening.
  The obsolete global clock/battery visibility preference is absent.

Debug mouse, pen and touch journeys pass. Release catalog and Preferences checks
pass. Captures are reviewed for geometry, spacing, corners and bank visibility.
These use OS-delivered synthetic pointers and native UI Automation, not physical
stylus measurements.

## Shared header surfaces

At `f20611a`, light menus share one rounded half-opacity background; the title,
buttons and status tiles use the same surface. The workspace switcher uses the
shared palette's tab-bar color. Eight full Photo scene pairs cover 744 and 1200
logical pixels, both themes and fitted/paper-under-header cameras at scale 1.5.
Five sampled backgrounds match the current Web output exactly: menu/title/settings
RGB 219 and switcher RGB 222. The native captures preceded only the integrated
shader syntax rebuild; the full Release header fixture used the final runtime.

All eight menus open and dismiss with mouse and injected pen/touch; File also
passes Space/Enter activation, for 26 checks with the document preserved. The full
Release Paint catalog/Preferences and header customization journey passes.
The strict image comparison still reports the existing 21-physical-pixel header
width difference and compact control-set differences. Full captures are retained;
these sample and interaction results do not establish whole-editor pixel parity.

## Reproduce

Run native GUI fixtures serially. Each run owns a disposable profile and its app
processes; trace discovery rejects files older than the current process.

~~~powershell
./apps/layer-windows/scripts/build.ps1 -Configuration Release -SkipRestore
./apps/layer-windows/scripts/exercise-header.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -Workspace sketch -Device pen -Catalog
./apps/layer-windows/scripts/exercise-header.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -Workspace sketch -Device touch
./apps/layer-windows/scripts/exercise-header.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -Workspace paint -Device mouse
cargo test --locked -p layer-core -p layer-engine -p layer-ui -p layer-host -p layer-workspace -p layer-windows --lib
cargo clippy --locked -p layer-core -p layer-engine -p layer-ui -p layer-host -p layer-workspace -p layer-windows --all-targets -- -D warnings
~~~

At the original title-bar checkpoint, ordinary Rust suites passed 678 tests;
hardware/diagnostic cases
remain explicitly ignored. Strict Clippy, Release compilation, native input
queue tests and presentation-analysis tests pass. Existing SDK library-search
warnings are environmental.

## Remaining scope

This milestone does not complete whole-editor Web/Android parity or physical
pressure/tilt/eraser, mixed-display and suspend/resume acceptance. Physical 120 Hz
painting and input-to-present latency still require the unavailable high-refresh
display. Portable packaging is refreshed separately; signed MSIX installation,
update/uninstall and clean-machine acceptance remain open.
