# Arch Linux packaging

`capycanvas-git/PKGBUILD` builds the current upstream revision for Arch Linux
and Omarchy. It stages the same desktop entry, MIME registration, icon, filters,
license material and pinned GTK runtime as the native Linux package. Photo
codecs are compiled into the shared Rust core.

The recipe no longer downloads or builds native photo-codec archives. `shaderc`
supplies GTK's shader compiler; `cargo-about` collects original Rust dependency
notices. GLib tools, DRM/Vulkan headers and Wayland protocols are explicit GTK
build dependencies. GTK runtime sources and missing license notices use the pinned checksums
and revisions from the shared packaging tools.

Rust copyright notices are collected from either rustup's sysroot or Arch's
system `rust` package. The native packager removes application debug data; additional
`makepkg` stripping is disabled so the bundled GTK library still matches its
shipped checksum manifest.

Validate the package from a clean copy of this directory:

```bash
makepkg --syncdeps --cleanbuild --clean --noconfirm
namcap capycanvas-git-*.pkg.tar.zst
```

Release packages are published on the GitHub Releases page. Verify and install
a downloaded package with:

```bash
sha256sum -c capycanvas-git-*.pkg.tar.zst.sha256
sudo pacman -U capycanvas-git-*.pkg.tar.zst
```

## VM qualification (2026-09-20 UTC)

The recipe was built and installed in the official Arch Linux
`Arch-Linux-x86_64-cloudimg-20260915.594445.qcow2` image, fully updated with
`pacman -Syu`, under QEMU 10.2.2/KVM (24 vCPUs, 48 GiB RAM). The guest used
Linux 7.2.6, Rust 1.98.1, GTK 4.22.5, libadwaita 1.9.4, Hyprland 0.56.2 and
Mesa/Venus 26.2.3. Sources were upstream `a23c627a` plus the packaging and
test-fixture fixes in this change.

- `makepkg` passed all 493 shared UI and 22 Linux tests. The Linux suite's 184
  opt-in integration tests remained ignored by the recipe.
- `pacman -U` installed successfully; `pacman -Qkk` reported 46 files and zero
  altered files. Desktop-entry validation and every bundled GTK manifest hash
  passed. `namcap` reported warnings about retained symbols and implicitly
  satisfied GTK dependencies, but no errors.
- The installed desktop entry launched `/usr/bin/capycanvas-bin` on hardware
  Vulkan through Venus, with mailbox presentation and managed Display P3.
  Wayland virtual-pointer input drew a house, tree and sun. The saved `.capy`
  picture rendered identically across repeated launches and its tiles were also
  independently decoded. An additional stroke rendered correctly; undo restored
  the original canvas exactly, and redo differed by at most 2/255 per display
  channel after restoring the 8-bit backing.

The NVIDIA-backed Venus setup required synchronous driver operations for reliable
rendering across launches. Start the **guest compositor** with this environment
when reproducing this VM test:

```bash
VN_PERF=no_async_set_alloc,no_async_buffer_create,no_async_queue_submit,no_async_mem_alloc,no_async_image_create,no_async_image_format,no_async_present,no_second_queue Hyprland
```

Default asynchronous Venus operation produced intermittent black or partial
frames. These are VM test settings, not package defaults or application renderer
changes. This qualifies installation and basic drawing on Arch/Hyprland; it is
not a full Omarchy installation or a performance/tablet/HDR qualification.
