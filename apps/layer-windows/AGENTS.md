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
- Workspace preferences are serialized by shared Rust. Keep native switcher
  updates driven by its acknowledgement and cross-window refresh.
- Native drag previews may complete after newer motion arrives. Publish replies
  for the active gesture while requesting the latest position, and require a
  current result for the final drop.
- Panel tabs use `ScrollView` with touch and pen scrolling disabled: those
  contacts belong to tab dragging. Keep wheel scrolling and preview clipping
  when changing the strip, and run all `tab-pickup` variants plus `tab-drag`.
