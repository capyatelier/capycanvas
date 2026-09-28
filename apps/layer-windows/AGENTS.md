# Windows client

Start with the [Windows guide](../../docs/development/windows.md) and the
[porting guide](../../docs/WINDOWS_PORTING_GUIDE.md). From Linux, use
[`tools/windows-vm/windows-vm.py`](../../docs/development/windows-vm.md).

- Build with `scripts/build.ps1`; MSBuild alone copies a stale Rust DLL.
- The host reads snapshot JSON by field name, so renaming a shared field fails
  silently here. Search this client for the old name.
- Follow the [fixture rules](../../docs/development/windows.md#ui-fixtures) for
  operation acknowledgements, arranged pointer targets and composed GPU captures.
  The [VM runner](../../docs/development/windows-vm.md#ui-fixtures-on-the-software-adapter)
  rejects stale `--no-build` binaries and incomplete fixture plans; retain its
  provenance and per-fixture evidence when reporting failures.
- Run `scripts/test-without-gpu.ps1` for the Windows checks. Each invocation
  gives the document tests a fresh settings directory; reusing an earlier
  directory invalidates their storage-failure setup.
- Selection fixtures follow the shared command IDs and labels. Stored preference
  reads retain valid fields, default invalid fields without an error, and leave
  the file unchanged until the next edit. Write failures still require recovery.
- Undo and Redo during selection completion are queued in shared Rust until
  the GPU result enters history. Keep the native selection fixture exercising
  immediate Undo after the presented Subtract outline.
- GPU surface replacement retains live contacts so shared Rust can reconstruct
  the active stroke. Cancel them only for actual input cancellation or exhausted
  recovery. The isolated device-loss hook drains rendering and presentation on
  every participating render worker before removing the shared device; keep
  these waits out of normal frames. The default VM suite includes both document
  recovery variants. Raw in-flight removal tests still require hardware.
- Workspace preferences are serialized by shared Rust. Keep native switcher
  updates driven by its acknowledgement and cross-window refresh.
- Native drag previews may complete after newer motion arrives. Publish replies
  for the active gesture while requesting the latest position, and require a
  current result for the final drop.
- Map `PointerPointProperties.IsCanceled` to cancellation before forwarding
  samples or feeding the hold recognizer. A canceled release must not complete
  a stroke or trigger a two-finger Undo tap.
- Panel tabs use `ScrollView` with touch and pen scrolling disabled: those
  contacts belong to tab dragging. Keep wheel scrolling and preview clipping
  when changing the strip, and run all `tab-pickup` variants plus `tab-drag`.
