# Arch Linux packaging

`capycanvas-git/PKGBUILD` builds the current upstream revision for Arch Linux
and Omarchy. It stages the same desktop entry, MIME registration, icon, filters,
license material, and pinned HEIF/AVIF codec bundle as the native Linux package.

The codec archives are ordinary `makepkg` sources with checked hashes. The
build therefore does not fetch unmanaged sources from inside `build()`.

Until upstream PR #2 lands, the recipe applies its immutable commit to restore
the native build and add the protected-Paper guidance. The guarded patch step
becomes a no-op once those changes are present upstream.

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
