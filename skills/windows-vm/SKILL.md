---
name: windows-vm
description: Start, reserve, connect to, and test Capy Canvas Windows VMs from Linux, with optional exclusive hardware GPUs and preserved test evidence.
---

# Windows VM work

Use the repository's `tools/windows-vm/windows-vm.py` rather than constructing
QEMU commands. Read [the VM guide](../../docs/development/windows-vm.md) for host
setup, test modes and evidence paths, and
[Windows testing](../../docs/development/windows.md) for client-specific journeys.
Run the helper from the assigned checkout: its own path selects the source tree
and default VM name. Check `CAPYCANVAS_VM` before relying on that default; use an
explicit owned VM name when checkout basenames collide.

Use the task's worktree VM by default; `--vm NAME` selects a separately owned VM.
`list` shows VM state. Preserve other sessions' VMs, installs, processes and GPUs.
Keep large images, build caches and captures on local SSD storage; respect the
configured state/artifact directories and prefer `/mnt/localssd` when available.

## Choose and reserve

Builds and model/input checks use ordinary `check`; software UI journeys use
`fixtures`. WARP results do not establish hardware rendering or performance.

For hardware rendering:

```sh
tools/windows-vm/windows-vm.py gpus
tools/windows-vm/windows-vm.py start --gpu
tools/windows-vm/windows-vm.py sync
tools/windows-vm/windows-vm.py ssh 'Get-PnpDevice -Class Display'
```

The helper reserves one free pool card at boot. QEMU keeps its lock until actual
shutdown, including after a launcher/client disconnect. A reserved or external
card is unavailable. Do not clear lock files, kill another VM or fall back to a
software adapter to satisfy a hardware request. A running VM cannot change its
GPU; hot-attach is not part of this workflow.

Routine VM startup needs no host driver changes. When the pool is absent or
unavailable, inspect `gpus` and the host setup guide. Host setup uses explicitly
chosen spare cards and existing user authorization; it must not be inferred from
an ordinary build/test request. Preserve any running GPU VM during host setup.

## Verify the real app

```sh
tools/windows-vm/windows-vm.py gpu-check
tools/windows-vm/windows-vm.py fixtures --hardware-gpu canvas-editing documents:default layers:default
```

The hardware suite uses a normal optimized app build with software adapters
disabled. Require actual hardware-adapter evidence from rendered fixture traces.
Walk the relevant light/dark variants and real drawing/document journeys for the
change. Startup-failure fixtures that intentionally create no renderer belong in
the ordinary fixture matrix. Keep per-fixture captures, source/binary provenance,
adapter identity and every failure with the result; a successful process exit or
an empty test selection is insufficient.

`--no-build` reuses binaries only after the helper validates their inputs and
hashes. Keep the VM warm between edits to reuse Cargo, NuGet and native caches.
The GPU suite proves rendering behaviour; physical input, HDR presentation,
display combinations and frame-rate targets have their own hardware requirements.

## Release

```sh
tools/windows-vm/windows-vm.py stop
tools/windows-vm/windows-vm.py gpus
```

Stop the owned VM when its GPU is no longer needed. A shutdown timeout preserves
the VM and reservation for diagnosis. Do not force QMP quit, unbind a live GPU or
repeat PCI resets; an unclean teardown can leave hardware needing host recovery.
Report any unverified recovery or hardware behaviour explicitly.
