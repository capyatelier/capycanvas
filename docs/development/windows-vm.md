# Windows VMs on Linux

[Windows development](windows.md) · [Developer guide](README.md)

`tools/windows-vm/windows-vm.py` runs Windows 11 Enterprise evaluation VMs under
QEMU/KVM. They have no hardware GPU, so they run the Windows tests on Windows'
software D3D12 adapter (WARP):

| Runs in a VM | Needs a hardware GPU |
| --- | --- |
| `check`: the build, native input tests and Rust unit tests, including the GPU-backed `layer-host` tests | `layer-render-wgpu` unit tests, which time out and exceed pixel tolerances on WARP |
| `fixtures`: the native UI fixtures | ignored `layer-windows` tests, which remove devices or need an explicitly selected adapter |
| | `tools/performance` scripts, PresentMon, physical pens and touch screens, HDR output, multiple displays and visual parity captures |

Requirements: x86_64 Linux with KVM, Python 3, about 150 GB of free disk space for
the base image plus 10–30 GB for each VM, and network access. `setup` installs
packages automatically on Fedora, Ubuntu/Debian and Arch.

```sh
tools/windows-vm/windows-vm.py setup     # packages, KVM access, ISO download
tools/windows-vm/windows-vm.py create    # unattended Windows and toolchain install
tools/windows-vm/windows-vm.py check     # sync, build and run GPU-free tests
tools/windows-vm/windows-vm.py fixtures  # sync, build and run the UI fixtures on WARP
```

`setup` downloads the evaluation ISO into your downloads folder unless it is
already there; `--iso` uses an existing file. `create` installs Windows, OpenSSH,
Visual Studio Build Tools, Rust, NuGet and PowerShell 7 into a read-only base
image. It needs no input and usually takes 20–40 minutes; pass `--gui` to watch it.

## VMs

Every other command acts on one VM made from the base image. `--vm NAME` or
`CAPYCANVAS_VM` selects it; the default is the worktree's directory name, so each
worktree has its own VM and its own `C:\capycanvas`. The first `start` creates the
VM as a copy-on-write disk over the base image and prepares it with
[`prepare.ps1`](../../tools/windows-vm/prepare.ps1): PowerShell 7 if the base lacks
it, a 2560 × 1600 display at 150 % scale, the dark theme the fixtures were
written against, and one restart. This takes a few
minutes; later starts take under a minute. Several VMs can run at once, each with
its own disk, TPM, SSH port and desktop session.

```sh
tools/windows-vm/windows-vm.py --vm review start
tools/windows-vm/windows-vm.py list
```

`check` boots the VM when needed, copies the working tree, including uncommitted
and untracked files, to `C:\capycanvas`, and runs
[`test-without-gpu.ps1`](../../apps/layer-windows/scripts/test-without-gpu.ps1):
the Debug build (`--release` for Release), the native input tests, and the
shared and Windows Rust unit tests, with the `layer-host` GPU tests on WARP. Build
outputs persist between syncs. Run other commands from the synced tree, for example strict Clippy:

```sh
tools/windows-vm/windows-vm.py ssh 'cd C:\capycanvas; cargo clippy --locked -p layer-windows --all-targets -- -D warnings'
```

| Command | Effect |
| --- | --- |
| `start [--gui]`, `stop` | Boot or shut down the VM, creating it first if needed. |
| `list` | Show the VMs and which are running. |
| `ssh [command]` | Open PowerShell, or run a command, as the `capy` administrator. |
| `sync` | Copy the working tree without building. |
| `fixtures [name ...]` | Run UI fixtures on WARP; see below. |
| `screenshot <file.png>` | Save the VM display. |
| `reset` | Delete the VM; the next `start` makes a fresh one. |
| `destroy` | Delete the base image and every VM; the ISO stays. Stop the VMs first. |

State lives in `~/.local/share/capycanvas/windows-vm` (`CAPYCANVAS_VM_DIR`): the
base image in `base` and the VMs in `vms`. `CAPYCANVAS_VM_CPUS` (default up to 8),
`CAPYCANVAS_VM_MEMORY` (default `16G`) and `CAPYCANVAS_VM_DISPLAY` (default
`2560x1600`) apply when a VM starts. The generated account password is in
`password` there.

Windows Update is disabled so the VMs keep the ISO's build. The evaluation
licence expires 90 days after `create`; stop every VM, then run `destroy` and
`create` to renew it.

## UI fixtures on the software adapter

`fixtures` syncs, builds Release with `build.ps1 -SoftwareAdapterTests` into
`artifacts\windows\SoftwareAdapter`, and runs
[`run-fixtures.ps1`](../../tools/windows-vm/run-fixtures.ps1) in the VM's desktop
session through a scheduled task, because SSH sessions cannot use UI Automation
or inject input. The runner works like a contributor at an unlocked desktop: one
fixture at a time, from a visible console that gives each fixture foreground
rights. It stops leftover CapyCanvas processes between runs and records each
exit code, duration and final error. It sets `CAPY_WAIT_SCALE=3`, which
lengthens every `CapyUia.ps1` wait for the slower adapter. Name fixtures
(`layers`) or single variants (`header:pen`); with no names it runs `shortcuts`
and every fixture that launches its own app, once with its defaults and once for
each switch and each other `ValidateSet` choice. `--no-build` reuses the last
build. Results, logs and failure screenshots are copied to
`artifacts/windows-vm/<vm>/<run>/`.

To use several VMs, give each a share of the names:

```sh
tools/windows-vm/windows-vm.py --vm fixtures-a fixtures layers palettes proof &
tools/windows-vm/windows-vm.py --vm fixtures-b fixtures header selection editor &
```

Only builds with the `software-adapter-tests` feature of `layer-windows` accept a
software adapter, and only while `LAYER_TEST_SOFTWARE_GPU` is set. Shipped builds
never enable the feature and still refuse CPU adapters with "Painting requires a
hardware D3D12 GPU". UI traces name the adapter in `windows_adapter`, for example
`Microsoft Basic Render Driver (Cpu)`.

A WARP pass shows that the UI structure, shared state, injected mouse, pen and
touch routing, persistence, windows and device-loss handling behave as the
fixture expects. It does not validate hardware drivers, frame pacing or latency,
physical pens and touch screens, HDR output, or mixed-DPI and multiple displays.
WARP timings mean nothing; run `tools/performance` scripts and PresentMon captures
on hardware. `exercise-package.ps1` is not run because the VM copy has no Git
metadata for `package.ps1`.
