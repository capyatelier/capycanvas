#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"
state=$(realpath -m "${CAPY_FLATPAK_BUILD_DIR:-target/flatpak}")
export FLATPAK_USER_DIR=${FLATPAK_USER_DIR:-$state/installation}
test -f "$state/.capy-flatpak-build"
test -f "$state/app/metadata"
output="$root/dist/flatpak"
if [[ -e "$output" && ! -f "$output/.capy-flatpak-package" ]]; then
    echo "Refusing to replace an unmarked Flatpak package directory: $output" >&2
    exit 1
fi
rm -rf "$output"
mkdir -p "$output"
touch "$output/.capy-flatpak-package"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml)
url=${CAPY_FLATPAK_REPO_URL:-https://capyatelier.github.io/capycanvas/flatpak}
key="$state/release-key.gpg"
gpg --batch --yes --dearmor --output "$key" packaging/flatpak/release-key.asc
sign=()
origin=()
if [[ ${SIGNED:-false} == true ]]; then
    : "${FLATPAK_GPG_PRIVATE_KEY:?}" "${FLATPAK_GPG_PASSPHRASE:?}" "${FLATPAK_GPG_FINGERPRINT:?}"
    signing_home=$(mktemp -d "${RUNNER_TEMP:-$state}/flatpak-gpg.XXXXXX")
    trap 'gpgconf --homedir "$signing_home" --kill gpg-agent; rm -rf "$signing_home"' EXIT
    printf '%s\n' allow-preset-passphrase > "$signing_home/gpg-agent.conf"
    printf '%s\n' "$FLATPAK_GPG_PRIVATE_KEY" | gpg --homedir "$signing_home" --batch --import
    gpg --homedir "$signing_home" --batch --export "$FLATPAK_GPG_FINGERPRINT" > "$signing_home/public.gpg"
    cmp "$key" "$signing_home/public.gpg"
    while read -r grip; do
        printf '%s\n' "$FLATPAK_GPG_PASSPHRASE" | "$(gpgconf --list-dirs libexecdir)/gpg-preset-passphrase" \
            --homedir "$signing_home" --preset "$grip"
    done < <(gpg --homedir "$signing_home" --batch --with-colons --with-keygrip --list-secret-keys "$FLATPAK_GPG_FINGERPRINT" | awk -F: '$1 == "grp" {print $10}')
    sign=(--gpg-homedir="$signing_home" --gpg-sign="$FLATPAK_GPG_FINGERPRINT")
    origin=(--repo-url="$url" --gpg-keys="$key")
fi
flatpak build-export "${sign[@]}" --collection-id=art.capycanvas.Stable "$state/repository" "$state/app" stable
flatpak build-update-repo "${sign[@]}" --title='Capy Canvas' --default-branch=stable \
    --generate-static-deltas --prune --prune-depth=2 "$state/repository"
flatpak build-bundle "${origin[@]}" --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    "$state/repository" "$output/capycanvas-$version-linux-x86_64.flatpak" art.capycanvas.CapyCanvas stable
if [[ ${SIGNED:-false} == true ]]; then
    python3 - "$key" "$url" "$output/capycanvas.flatpakref" <<'PY'
import base64, pathlib, sys
key, url, output = sys.argv[1:]
pathlib.Path(output).write_text('\n'.join([
    '[Flatpak Ref]', 'Name=art.capycanvas.CapyCanvas', 'Branch=stable',
    'Title=Capy Canvas', 'IsRuntime=false', 'SuggestRemoteName=capycanvas',
    'Url=' + url, 'RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo',
    'GPGKey=' + base64.b64encode(pathlib.Path(key).read_bytes()).decode(), '',
]))
PY
    tar --zstd -cf "$output/capycanvas-$version-flatpak-repository.tar.zst" -C "$state" repository
fi
