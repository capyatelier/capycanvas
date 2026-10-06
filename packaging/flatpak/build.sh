#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"
state=$(realpath -m "${CAPY_FLATPAK_BUILD_DIR:-target/flatpak}")
if [[ -d "$state" && ! -f "$state/.capy-flatpak-build" ]]; then
    echo "Refusing to replace an unmarked Flatpak build directory: $state" >&2
    exit 1
fi
mkdir -p "$state"
touch "$state/.capy-flatpak-build"
export FLATPAK_USER_DIR=${FLATPAK_USER_DIR:-$state/installation}
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive flathub org.gnome.Sdk//50 org.gnome.Platform//50 org.freedesktop.Sdk.Extension.node24//25.08
rm -rf "$state/app" "$state/source"
mkdir "$state/source"
git ls-files -z | tar --null -T - -cf - | tar -xf - -C "$state/source"
flatpak build-init --sdk-extension=org.freedesktop.Sdk.Extension.node24 \
    "$state/app" art.capycanvas.CapyCanvas org.gnome.Sdk org.gnome.Platform 50
rust_version=${RUST_VERSION:-$(sed -n 's/^  RUST_VERSION: "\(.*\)"$/\1/p' .github/workflows/release.yml)}
flatpak build --share=network --filesystem="$state" \
    --bind-mount="/run/build/capycanvas=$state/source" --build-dir=/run/build/capycanvas \
    --env="RUSTUP_HOME=$state/rustup" --env="CARGO_HOME=$state/cargo" \
    --env="CARGO_TARGET_DIR=$state/cargo-target" --env="CAPY_GTK_BUILD_DIR=$state/gtk-runtime" \
    --env="PATH=$state/cargo/bin:/usr/lib/sdk/node24/bin:/app/bin:/usr/bin" \
    --env="RUST_VERSION=$rust_version" --env="CAPY_BUILD_JOBS=${CAPY_BUILD_JOBS:-4}" \
    --env=CARGO_TERM_COLOR=never "$state/app" bash packaging/flatpak/compile.sh
sed 's/^name=art.capycanvas.CapyCanvas$/name=art.capycanvas.CapyCanvas.Devel/' \
    "$state/app/metadata" > "$state/app/metadata-compose"
flatpak build --metadata=metadata-compose "$state/app" appstreamcli compose \
    --no-net --prefix=/ --origin=art.capycanvas.CapyCanvas \
    --result-root=/app --data-dir=/app/share/app-info/xmls \
    --icons-dir=/app/share/app-info/icons/flatpak --components=art.capycanvas.CapyCanvas /app
rm "$state/app/metadata-compose"
test -s "$state/app/files/share/app-info/xmls/art.capycanvas.CapyCanvas.xml.gz"
flatpak build-finish --command=capycanvas --socket=wayland --device=dri "$state/app"
