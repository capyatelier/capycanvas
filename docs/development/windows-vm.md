# Windows VM on Linux

[Windows development](windows.md) · [Developer guide](README.md)

`tools/windows-vm/windows-vm.py` runs a Windows 11 Enterprise evaluation VM under
QEMU/KVM. It builds the Windows client and runs the tests that need no GPU. The
VM has no hardware D3D12 adapter, so it cannot launch the app, run `exercise-*.ps1`
fixtures or run the ignored hardware tests.

Requirements: x86_64 Linux with KVM, Python 3, about 150 GB of free disk space and
network access. `setup` installs packages automatically on Fedora, Ubuntu/Debian
and Arch.

```sh
tools/windows-vm/windows-vm.py setup    # packages, KVM access, ISO download
tools/windows-vm/windows-vm.py create   # unattended Windows and toolchain install
tools/windows-vm/windows-vm.py check    # sync, build and run GPU-free tests
```

`setup` downloads the evaluation ISO into your downloads folder unless it is
already there; `--iso` uses an existing file. `create` installs Windows, OpenSSH,
Visual Studio Build Tools, Rust and NuGet into a base image. It needs no input and
usually takes 20–40 minutes; pass `--gui` or run `screenshot <file.png>` to watch it.

`check` boots the VM when needed, copies the working tree, including uncommitted
and untracked files, to `C:\capycanvas`, and runs
[`test-without-gpu.ps1`](../../apps/layer-windows/scripts/test-without-gpu.ps1):
the Debug build (`--release` for Release), the native input and presentation
analysis tests, and the shared and Windows Rust unit tests. Build outputs persist
between syncs. Run other commands from the synced tree, for example strict Clippy:

```sh
tools/windows-vm/windows-vm.py ssh 'cd C:\capycanvas; cargo clippy --locked -p layer-windows --all-targets -- -D warnings'
```

| Command | Effect |
| --- | --- |
| `start [--gui]`, `stop` | Boot or shut down the VM. |
| `ssh [command]` | Open PowerShell, or run a command, as the `capy` administrator. |
| `sync` | Copy the working tree without building. |
| `reset` | Discard every change since `create`. |
| `destroy` | Delete the VM; the ISO stays. |

State lives in `~/.local/share/capycanvas/windows-vm` (`CAPYCANVAS_VM_DIR`). Set
`CAPYCANVAS_VM_CPUS` (default up to 8) and `CAPYCANVAS_VM_MEMORY` (default `16G`)
before starting the VM. The generated account password is in `password` there.

Windows Update is disabled so the VM keeps the ISO's build. The evaluation
licence expires 90 days after `create`; run `destroy` and `create` to renew it.
