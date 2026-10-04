#!/usr/bin/env bash
# Run as root in a disposable Arch Linux container; it installs packages into /usr.
set -euo pipefail
cd "$(dirname "$0")/../.."
source packaging/arch/capycanvas-git/PKGBUILD
sed -i '/^NoExtract/d' /etc/pacman.conf
pacman -Syu --noconfirm $(pacman -Qq)
pacman -S --noconfirm --needed base-devel patchelf wget zsync "${depends[@]}" "${makedepends[@]}"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml)
grep -q "<release version=\"$version\"" apps/layer-linux/art.capycanvas.CapyCanvas.metainfo.xml ||
  { echo "Add a $version release to the AppStream metainfo" >&2; exit 1; }
node apps/layer-linux/package.mjs
cp -a dist/capycanvas-linux/{bin,lib,share} /usr/
wget -qO target/quick-sharun https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/cb3a7cc48eba770d3eedab02bb63eaff7e8fe572/useful-tools/quick-sharun.sh
chmod +x target/quick-sharun
export DESKTOP=/usr/share/applications/art.capycanvas.CapyCanvas.desktop
export ICON=/usr/share/icons/hicolor/scalable/apps/art.capycanvas.CapyCanvas.svg
export APPDIR=target/AppDir OUTPATH=dist OUTNAME="capycanvas-$version-linux-x86_64.AppImage"
export UPINFO="gh-releases-zsync|capyatelier|capycanvas|latest|capycanvas-*-linux-x86_64.AppImage.zsync"
target/quick-sharun /usr/bin/capycanvas
docs=target/AppDir/share/doc
mkdir -p "$docs/system-libraries" target/AppDir/share/metainfo
cp /usr/share/metainfo/art.capycanvas.CapyCanvas.metainfo.xml target/AppDir/share/metainfo/
cp -a /usr/share/doc/capycanvas /usr/share/doc/capycanvas-gtk "$docs/"
cp -a /usr/share/licenses/spdx "$docs/system-libraries/"
{ find target/AppDir/lib -name '*.so*' -printf '/usr/lib/%P\n' | xargs pacman -Qqo 2>/dev/null || true; } |
  sort -u | while read -r package; do
    pacman -Qi "$package" | grep -E '^(Name|Version|Licenses|URL) ' >> "$docs/system-libraries/packages.txt"
    [[ ! -d /usr/share/licenses/$package ]] || cp -rL "/usr/share/licenses/$package" "$docs/system-libraries/"
  done
target/quick-sharun --make-appimage
