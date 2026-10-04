# Capy Canvas for Windows

The native Windows client: WinUI 3 controls in C++/WinRT around the shared Rust
editor, drawing with the shared wgpu renderer on D3D12. One GPU canvas covers the
whole client area, including behind the custom title bar.

- Build, run, test, package and debug: [Windows development](../../docs/development/windows.md).
- Linux VMs for builds and UI fixtures: [Windows VMs](../../docs/development/windows-vm.md).
- Bringing upstream features to Windows: [porting guide](../../docs/WINDOWS_PORTING_GUIDE.md).

## Source layout

| Path | Contents |
| --- | --- |
| `App.cpp` | Entry point; opens windows and hands every later launch of the installation to its running instance. |
| `CanvasWindow.*` | One editor window: title bar, swap-chain attachment, independent canvas input, the render thread and the queues between them. |
| `CanvasWorkBuffer.h`, `CanvasQueryQueue.h`, `CanvasSnapshotMailbox.h` | Bounded, ordered input and command queue; optional UI queries; latest published models. |
| `WorkspaceView.*`, `PanelBody.*`, `WorkspaceDrawers.*`, `CollapsedColumns.*`, `WorkspaceExpansion.*` | Native projection of the shared workspace: panels, tabs, drawers, columns, panel configuration. |
| `WorkspaceGestures.*`, `WorkspaceTabDrag.*`, `WorkspaceRowDrag.*`, `LayerRowDrag.*`, `HeaderInput.*` | Native drag and hold arbitration for the [drag convention](../../docs/ui/drag-and-reorder.md). |
| `HeaderView.*`, `HeaderStatus.h`, `DrawingTabs.h`, `NativeMenus.h`, `CommandSearch.h` | Title bar, menus, drawing tabs and command search. |
| Other `*View.*`, `*Form.h`, `*Control.*` | Panels, dialogs and controls: layers, color, palettes, tools, filters, effects, Navigator, Proof, Preferences. |
| `native/` | The `layer-windows` Rust crate, built as `layer_windows.dll`. `native/include/capy_windows.h` is the C ABI. |
| `native/src/host.rs` | Render owner: device, surface, frame loop and the exported entry points around `NativeHost`. |
| `native/src/documents.rs`, `document_*.rs`, `recovery.rs` | Document worker: open, save, export, drawing tabs, color and proof workflows, crash recovery. |
| `native/src/workspace*.rs`, `settings.rs`, `storage.rs` | Workspace storage on a SQLite worker, preferences, and the installation's [folders](../../docs/development/windows.md#where-files-live). |
| `native/src/filter_packages.rs`, `device.rs`, `display.rs` | Runtime filter transport, device-removal detection, display HDR state. |
| `tests/` | C++ queue test (`scripts/test-input.ps1`) and the Color and Number control review entry points. |
| `scripts/` | `build.ps1`, packaging, asset staging, `exercise-*.ps1` UI fixtures and their UI Automation helpers (`CapyUia.ps1`). |
| `CapyCanvas.vcxproj`, `packages.config`, `packaging/` | MSBuild project, pinned NuGet packages, third-party notices. |

## Where to start reading

1. `CanvasWindow.h`, then `Start`, `StartInput` and `Run` in `CanvasWindow.cpp`:
   which thread owns what.
2. `native/include/capy_windows.h` and `native/src/lib.rs`: the boundary between
   WinUI and Rust.
3. `native/src/host.rs`: the frame loop and presentation.
4. `WorkspaceView.cpp` and `PanelBody.cpp`: how shared snapshots become retained
   native controls.

## How the host works

- **Rust owns behavior.** Business rules, validation, layout, history and storage
  policy are shared Rust. WinUI presents shared snapshots, measures native
  controls and captures input. Snapshots are JSON read by field name and actions
  are sent as strings, so a renamed shared field fails silently.
- **Startup.** The render owner reads saved preferences before shared Rust
  creates the launch context and first UI view. It parks while the XAML thread
  adopts the prepared host and attaches the surface. The profile settings hub
  prepares the latest language on its worker and shares it with existing and
  future windows before publishing coherent catalog, bootstrap and view updates.
  An input owner defers publication until its composition keys and canvas
  contacts retire. Native controls receive its language tag and preserve IME
  composition before forwarding candidate keys. Stateless numeric and toolbar
  calls borrow the current immutable context retained by their native view owners,
  independently of the render-owned session.
- **Three threads.** The XAML UI thread owns controls and the window. An
  independent input source collects mouse, pen and touch with full history and
  `PointerPredictor` predictions on its own thread, so a presentation wait never
  stops input. The render thread exclusively owns the Rust host: commands, GPU
  work and presentation. UI code never waits for a frame.
- **Bounded queues.** Input and commands travel in one ordered, bounded queue; a
  full queue reports an error and cancels active input instead of dropping
  records. Optional queries have their own queue. Published models go through a
  mailbox with separate full-model, motion and camera slots. Motion packets
  (`workspace_update`) move native presentation only; stale revisions are
  rejected.
- **Input records.** Each record carries the camera revision captured with it;
  the render owner never retags it with a newer camera. Real samples are never
  smoothed or dropped; out-of-range predicted samples are clamped or discarded.
  Keys arrive on the XAML thread and pointers on the input thread, so a contact
  start carries its own modifiers and the host queues a modifier change ahead of
  it. The window drops Alt-only `SC_KEYMENU`, because menu mode stalls
  presentation until the next click.
- **Surface.** `SetSwapChain` runs on the UI thread while the render thread is
  parked; otherwise the render thread never waits on the UI dispatcher. The swap
  chain uses flip-discard buffers and maximum frame latency one. The render
  thread waits on the frame-latency object before draining the newest input, and
  presents immediately with tearing where supported, FIFO otherwise. It uses
  scRGB `Rgba16Float` when the output offers it.
- **Workers.** File, clipboard, profile, settings and SQLite work runs on workers;
  the render thread polls completions without blocking. Replaced GPU resources
  retire off the render thread.
- **Device loss.** A removed D3D12 device is rebuilt from the shared document.
  If that fails, painting stops, queued contacts are canceled, completed edits
  stay saveable and GPU-dependent commands are disabled.
- **Windows.** Each window owns its drawings, canvas, render thread, dialogs and
  workspace claim. Windows of one profile share preferences.
