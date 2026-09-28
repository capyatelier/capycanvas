# Windows client

Start with the [Windows guide](../../docs/development/windows.md) and the
[porting guide](../../docs/WINDOWS_PORTING_GUIDE.md). From Linux, use
[`tools/windows-vm/windows-vm.py`](../../docs/development/windows-vm.md).

- Build with `scripts/build.ps1`; MSBuild alone copies a stale Rust DLL.
- The host reads snapshot JSON by field name, so renaming a shared field fails
  silently here. Search this client for the old name.
